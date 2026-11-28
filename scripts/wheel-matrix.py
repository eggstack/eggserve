#!/usr/bin/env python3
"""Canonical wheel-matrix authority tools (Plans 265, 268).

Usage:
    python3 scripts/wheel-matrix.py validate [--matrix release/wheel-matrix.toml]
    python3 scripts/wheel-matrix.py emit-matrix [--matrix ...]  # GitHub Actions JSON
    python3 scripts/wheel-matrix.py expected-tags [--matrix ...] [--tier required]
    python3 scripts/wheel-matrix.py self-test

The manifest is the single authority for release wheel targets. The release
workflow and `check-release-wheel-set.py` consume it; no second manually
maintained platform list may remain.

Plan 268 Track A: `manylinux` (container/platform baseline) and
`compatibility` (maturin `--compatibility` policy) are separate controls.
Plan 268 Track B: cross-built artifacts use deferred smoke strategies and
must never claim build-host `native` smoke.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MATRIX = REPO_ROOT / "release" / "wheel-matrix.toml"

VALID_TIERS = {"required", "candidate", "deferred"}
VALID_BUILDS = {"native", "cross-container"}
# Plan 268 Track B: `native` = build-host install+execute; `deferred-native` =
# qualifier-host direct install; `deferred-container` = qualifier-host native
# container (AArch64 musl Alpine). `container` is a retained legacy spelling
# (no manifest entry uses it); `qemu` = emulated in-build smoke.
VALID_SMOKES = {
    "native", "deferred-native", "deferred-container",
    "qemu", "post-publish-only", "container",
}
DEFERRED_SMOKES = {"deferred-native", "deferred-container"}
VALID_LIBCS = {"glibc", "musl", "darwin", "msvc"}
VALID_MANYLINUX = {"2_17", "musllinux_1_2", "auto"}
VALID_COMPATIBILITY = {"pypi", "auto"}

# Plan 333: the matrix gains an ABI-family dimension. The GIL family stays
# cp311-abi3 on every existing target; the free-threaded family is
# cp314-cp314t on three targets only and is tier `required` so it must appear
# in every release aggregate (the public tier stays Beta).
VALID_ABI_FAMILIES = {"cp311", "cp314t"}
ABI_FAMILY_TAGS = {
    "cp311": ("cp311", "abi3"),
    "cp314t": ("cp314", "cp314t"),
}
FREE_THREADED_PLATFORM_TAGS = {
    "manylinux_2_17_x86_64",
    "macosx_11_0_arm64",
    "win_amd64",
}
FREE_THREADED_INTERPRETERS = {
    "python3.14t",
    "/opt/python/cp314-cp314t/bin/python",
}

RUST_TARGET_RE = re.compile(r"^[a-z0-9_]+-[a-z0-9_]+-[a-z0-9_]+(-[a-z0-9_]+)?$")
PLATFORM_TAG_RE = re.compile(r"^[A-Za-z0-9_]+$")

# x86_64 build hosts: a non-x86 arch claiming build-host native smoke is a
# cross-build routing defect (Plan 268 Track B).
X86_64_HOST_RUNNERS = {"ubuntu-latest", "windows-latest"}
CROSS_ARCH_FAMILIES = {"aarch64", "arm64", "armv7"}


def load_matrix(path: Path) -> dict:
    with open(path, "rb") as f:
        data = tomllib.load(f)
    return data


def validate_matrix(path: Path) -> list[str]:
    """Return a list of error strings (empty means valid)."""
    errors: list[str] = []
    try:
        data = load_matrix(path)
    except FileNotFoundError:
        return [f"matrix file not found: {path}"]
    except tomllib.TOMLDecodeError as exc:
        return [f"matrix TOML parse error: {exc}"]

    if data.get("authority_version") != 1:
        errors.append("authority_version must be 1")

    targets = data.get("target")
    if not isinstance(targets, list) or not targets:
        return errors + ["no [[target]] entries"]

    seen_ids: dict[str, int] = {}
    seen_tags: dict[tuple[str, str], str] = {}
    seen_rust: dict[tuple[str, str], str] = {}
    for i, t in enumerate(targets):
        where = f"target[{i}]"
        if not isinstance(t, dict):
            errors.append(f"{where}: not a table")
            continue
        tid = t.get("id", f"<missing #{i}>")
        for key in (
            "id", "name", "rust_target", "platform_tag", "runner",
            "manylinux", "compatibility", "build", "smoke", "tier",
            "arch_family", "libc", "artifact",
            "abi_family", "python_tag", "abi_tag", "interpreter",
            "setup_python",
        ):
            if key not in t:
                errors.append(f"{where} ({tid}): missing key {key!r}")
        # Reject the old conflated key if it reappears.
        if "maturin_compat" in t:
            errors.append(
                f"{where} ({tid}): stale key 'maturin_compat' "
                "(Plan 268: split into 'manylinux' + 'compatibility')"
            )
        tier = t.get("tier")
        if tier not in VALID_TIERS:
            errors.append(f"{where} ({tid}): unknown tier {tier!r}")
        if t.get("build") not in VALID_BUILDS:
            errors.append(f"{where} ({tid}): unknown build {t.get('build')!r}")
        if t.get("smoke") not in VALID_SMOKES:
            errors.append(f"{where} ({tid}): unknown smoke {t.get('smoke')!r}")
        if t.get("libc") not in VALID_LIBCS:
            errors.append(f"{where} ({tid}): unknown libc {t.get('libc')!r}")
        manylinux = t.get("manylinux")
        if manylinux not in VALID_MANYLINUX:
            errors.append(
                f"{where} ({tid}): unknown manylinux baseline {manylinux!r} "
                "(expected one of 2_17, musllinux_1_2, auto)"
            )
        compat = t.get("compatibility")
        if compat not in VALID_COMPATIBILITY:
            errors.append(
                f"{where} ({tid}): unknown compatibility {compat!r} "
                "(expected 'pypi' or 'auto')"
            )
        # Plan 333: the declared ABI family must agree with its tag triple,
        # interpreter, and (for free-threaded) the three-target subset.
        family = t.get("abi_family")
        if family not in VALID_ABI_FAMILIES:
            errors.append(
                f"{where} ({tid}): unknown abi_family {family!r} "
                f"(expected one of {sorted(VALID_ABI_FAMILIES)})"
            )
        else:
            expected_python, expected_abi = ABI_FAMILY_TAGS[family]
            if t.get("python_tag") != expected_python:
                errors.append(
                    f"{where} ({tid}): abi_family {family!r} needs "
                    f"python_tag {expected_python!r}, got "
                    f"{t.get('python_tag')!r}"
                )
            if t.get("abi_tag") != expected_abi:
                errors.append(
                    f"{where} ({tid}): abi_family {family!r} needs "
                    f"abi_tag {expected_abi!r}, got {t.get('abi_tag')!r}"
                )
            if family == "cp311":
                if t.get("interpreter") != "python3.11":
                    errors.append(
                        f"{where} ({tid}): cp311 family must build with "
                        "interpreter 'python3.11' "
                        f"(got {t.get('interpreter')!r})"
                    )
                if t.get("setup_python") != "3.11":
                    errors.append(
                        f"{where} ({tid}): cp311 family must set "
                        "setup_python '3.11' "
                        f"(got {t.get('setup_python')!r})"
                    )
            else:
                if t.get("platform_tag") not in FREE_THREADED_PLATFORM_TAGS:
                    errors.append(
                        f"{where} ({tid}): free-threaded target outside the "
                        "declared subset (Linux x86_64 glibc, macOS arm64, "
                        "Windows x86_64); got "
                        f"{t.get('platform_tag')!r}"
                    )
                if t.get("interpreter") not in FREE_THREADED_INTERPRETERS:
                    errors.append(
                        f"{where} ({tid}): free-threaded interpreter must be "
                        "python3.14t or the manylinux "
                        "/opt/python/cp314-cp314t/bin/python path "
                        f"(got {t.get('interpreter')!r})"
                    )
                if t.get("setup_python") != "3.14t":
                    errors.append(
                        f"{where} ({tid}): free-threaded setup_python must be "
                        "'3.14t' "
                        f"(got {t.get('setup_python')!r})"
                    )
        # Track A: never reuse the compatibility policy as the container selector.
        if manylinux == "pypi":
            errors.append(
                f"{where} ({tid}): compatibility 'pypi' reused as container "
                "selector (manylinux must be 2_17, musllinux_1_2, or auto)"
            )
        rust_target = str(t.get("rust_target", ""))
        if not RUST_TARGET_RE.match(rust_target):
            errors.append(f"{where} ({tid}): malformed rust target {rust_target!r}")
        tag = str(t.get("platform_tag", ""))
        if not PLATFORM_TAG_RE.match(tag):
            errors.append(f"{where} ({tid}): malformed platform tag {tag!r}")
        if tag.startswith("linux_"):
            errors.append(
                f"{where} ({tid}): generic linux_* tag {tag!r} "
                "(must be manylinux/musllinux family)"
            )
        libc = t.get("libc")
        # Track A: baseline/tag family consistency.
        if isinstance(tag, str) and isinstance(manylinux, str):
            if tag.startswith("manylinux_"):
                if manylinux != "2_17":
                    errors.append(
                        f"{where} ({tid}): manylinux target with no manylinux "
                        f"baseline (tag {tag!r}, manylinux={manylinux!r})"
                    )
                if t.get("libc") == "musl":
                    errors.append(
                        f"{where} ({tid}): musllinux target assigned a "
                        f"manylinux baseline (tag {tag!r})"
                    )
            elif tag.startswith("musllinux_"):
                if manylinux != "musllinux_1_2":
                    errors.append(
                        f"{where} ({tid}): musllinux target assigned a "
                        f"manylinux baseline (tag {tag!r}, "
                        f"manylinux={manylinux!r})"
                    )
                if t.get("libc") != "musl":
                    errors.append(
                        f"{where} ({tid}): musllinux tag on non-musl libc "
                        f"(tag {tag!r}, libc={t.get('libc')!r})"
                    )
            # Linux tag contradicts baseline family.
            if t.get("libc") == "glibc" and manylinux == "musllinux_1_2":
                errors.append(
                    f"{where} ({tid}): glibc target with musl baseline "
                    f"(tag {tag!r})"
                )
            if t.get("libc") == "musl" and manylinux == "2_17":
                errors.append(
                    f"{where} ({tid}): musllinux target assigned a manylinux "
                    f"baseline (tag {tag!r})"
                )
        # Non-Linux keeps the native baseline (auto); the PyPI policy travels
        # separately in `compatibility`.
        if t.get("libc") in ("darwin", "msvc") and manylinux != "auto":
            errors.append(
                f"{where} ({tid}): non-Linux target must keep native baseline "
                f"manylinux='auto' (got {manylinux!r})"
            )
        # Impossible smoke strategies.
        if t.get("smoke") == "qemu" and not t.get("qemu_platform"):
            errors.append(f"{where} ({tid}): qemu smoke needs qemu_platform")
        if t.get("smoke") == "qemu" and not t.get("qemu_image"):
            errors.append(f"{where} ({tid}): qemu smoke needs qemu_image")
        if t.get("build") == "native" and t.get("smoke") == "qemu":
            errors.append(
                f"{where} ({tid}): native build with qemu smoke is inconsistent"
            )
        # Track B: deferred routing must be explicit in the manifest.
        smoke = t.get("smoke")
        qualify_runner = t.get("qualify_runner", "")
        if qualify_runner and smoke == "native":
            errors.append(
                f"{where} ({tid}): cross-built target cannot claim build-host "
                "native smoke (use deferred-native/deferred-container with "
                f"qualify_runner={qualify_runner!r})"
            )
        if smoke in DEFERRED_SMOKES and not qualify_runner:
            errors.append(
                f"{where} ({tid}): deferred smoke {smoke!r} needs "
                "qualify_runner"
            )
        if (
            t.get("runner") in X86_64_HOST_RUNNERS
            and t.get("arch_family") in CROSS_ARCH_FAMILIES
            and smoke == "native"
        ):
            errors.append(
                f"{where} ({tid}): cross-arch {t.get('arch_family')!r} on "
                f"{t.get('runner')!r} cannot claim build-host native smoke "
                "(use deferred-native/deferred-container or qemu)"
            )
        # Duplicate detection.
        if tid in seen_ids:
            errors.append(f"duplicate target id {tid!r}")
        seen_ids[tid] = i
        # Plan 333: one entry per (platform_tag, abi_family); the same
        # platform may carry both families (cp311 + cp314t).
        family_key = t.get("abi_family", "")
        if tag and (tag, family_key) in seen_tags:
            errors.append(
                f"duplicate platform tag {tag!r} for family "
                f"{family_key!r} ({seen_tags[(tag, family_key)]!r} and "
                f"{tid!r})"
            )
            seen_tags[(tag, family_key)] = tid
        else:
            seen_tags[(tag, family_key)] = tid
        if rust_target and (rust_target, family_key) in seen_rust:
            errors.append(
                f"duplicate rust target {rust_target!r} for family "
                f"{family_key!r} ({seen_rust[(rust_target, family_key)]!r} "
                f"and {tid!r})"
            )
        else:
            seen_rust[(rust_target, family_key)] = tid

    required = [t for t in targets if isinstance(t, dict) and t.get("tier") == "required"]
    if not required:
        errors.append("no required targets declared")
    return errors


def required_tags(data: dict) -> list[str]:
    return sorted({
        t["platform_tag"] for t in data.get("target", [])
        if isinstance(t, dict) and t.get("tier") == "required"
    })


def required_pairs(data: dict) -> list[tuple[str, str]]:
    """Required (platform_tag, abi_family) pairs (Plan 333)."""
    return sorted(
        (t["platform_tag"], t["abi_family"]) for t in data.get("target", [])
        if isinstance(t, dict) and t.get("tier") == "required"
    )


def declared_tags(data: dict) -> list[str]:
    return sorted(
        t["platform_tag"] for t in data.get("target", [])
        if isinstance(t, dict) and "platform_tag" in t
    )


def emit_gha_matrix(data: dict) -> list[dict]:
    """Emit the release build matrix (required targets only)."""
    entries = []
    for t in data.get("target", []):
        if not isinstance(t, dict) or t.get("tier") != "required":
            continue
        runner = t["runner"]
        is_windows = runner.startswith("windows")
        entries.append({
            "name": f"Build wheel ({t['name']})",
            "id": t["id"],
            "os": runner,
            "target": t["rust_target"],
            # Plan 268 Track A: baseline and policy travel separately.
            # `manylinux` feeds the action's `manylinux:` container selector;
            # `compatibility` feeds `--compatibility` in the build args.
            "manylinux": t["manylinux"],
            "compatibility": t["compatibility"],
            "artifact": t["artifact"],
            "platform_tag": t["platform_tag"],
            "abi_family": t.get("abi_family", ""),
            "python_tag": t.get("python_tag", ""),
            "abi_tag": t.get("abi_tag", ""),
            "interpreter": t.get("interpreter", ""),
            "setup_python": t.get("setup_python", ""),
            "build": t["build"],
            "smoke": t["smoke"],
            "qemu_platform": t.get("qemu_platform", ""),
            "qemu_image": t.get("qemu_image", ""),
            "qualify_runner": t.get("qualify_runner", ""),
            "venv_python": (
                "/tmp/smoke-venv/Scripts/python" if is_windows
                else "/tmp/smoke-venv/bin/python"
            ),
            "console_script": (
                "/tmp/smoke-venv/Scripts/eggserve.exe" if is_windows
                else "/tmp/smoke-venv/bin/eggserve"
            ),
        })
    return entries


def cmd_self_test() -> int:
    """Parser self-tests: duplicates, tiers, triples, smoke strategies."""
    failures = 0

    def check(name: str, manifest: dict, expect_error_substr: str | None) -> None:
        nonlocal failures
        import tempfile
        with tempfile.NamedTemporaryFile(
            "wb", suffix=".toml", delete=False
        ) as tmp:
            # Minimal TOML writer for the self-test fixtures.
            tmp.write(b"authority_version = 1\n")
            for t in manifest.get("target", []):
                tmp.write(b"\n[[target]]\n")
                for k, v in t.items():
                    tmp.write(f'{k} = {json.dumps(v)}\n'.encode())
            tmp_path = Path(tmp.name)
        errs = validate_matrix(tmp_path)
        tmp_path.unlink(missing_ok=True)
        if expect_error_substr is None:
            if errs:
                print(f"FAIL {name}: expected valid, got {errs}")
                failures += 1
            else:
                print(f"OK {name}")
        else:
            if any(expect_error_substr in e for e in errs):
                print(f"OK {name}")
            else:
                print(f"FAIL {name}: expected {expect_error_substr!r}, got {errs}")
                failures += 1

    base = {
        "id": "t1", "name": "T1", "rust_target": "x86_64-unknown-linux-gnu",
        "platform_tag": "manylinux_2_17_x86_64", "runner": "ubuntu-latest",
        "manylinux": "2_17", "compatibility": "pypi",
        "build": "cross-container", "smoke": "native",
        "tier": "required", "arch_family": "x86_64", "libc": "glibc",
        "artifact": "wheel-a",
        "abi_family": "cp311", "python_tag": "cp311", "abi_tag": "abi3",
        "interpreter": "python3.11", "setup_python": "3.11",
    }

    def clone(**over: object) -> dict:
        d = dict(base)
        d.update(over)
        return d

    check("baseline valid", {"target": [clone()]}, None)
    other = clone(id="t2", platform_tag="manylinux_2_17_aarch64",
                  rust_target="aarch64-unknown-linux-gnu", artifact="wheel-b",
                  smoke="deferred-native", qualify_runner="ubuntu-24.04-arm")
    check("two-target valid", {"target": [clone(), other]}, None)
    free_threaded = clone(
        id="t1ft", abi_family="cp314t", python_tag="cp314",
        abi_tag="cp314t", interpreter="python3.14t",
        setup_python="3.14t", artifact="wheel-a-ft",
    )
    check("same platform carries both families",
          {"target": [clone(), free_threaded]}, None)
    check("unknown abi family",
          {"target": [clone(abi_family="cp313t")]}, "unknown abi_family")
    check("family tag mismatch",
          {"target": [clone(abi_family="cp314t")]}, "python_tag")
    check("free-threaded on musl",
          {"target": [clone(
              id="m", platform_tag="musllinux_1_2_x86_64",
              rust_target="x86_64-unknown-linux-musl",
              manylinux="musllinux_1_2", libc="musl",
              abi_family="cp314t", python_tag="cp314", abi_tag="cp314t",
              interpreter="python3.14t", setup_python="3.14t",
              artifact="wheel-m")]},
          "outside the declared subset")
    check("duplicate family pair",
          {"target": [free_threaded, clone(
              id="t2", abi_family="cp314t", python_tag="cp314",
              abi_tag="cp314t", interpreter="python3.14t",
              setup_python="3.14t", artifact="wheel-b-ft")]},
          "duplicate platform tag")
    check("duplicate ids", {"target": [clone(), clone(id="t1", platform_tag="win_amd64",
          rust_target="x86_64-pc-windows-msvc", artifact="wheel-c",
          manylinux="auto", libc="msvc", arch_family="x86_64")]}, "duplicate target id")
    check("duplicate tags", {"target": [clone(), clone(id="t2",
          rust_target="aarch64-unknown-linux-gnu", artifact="wheel-b")]}, "duplicate platform tag")
    check("unknown tier", {"target": [clone(tier="supported")]}, "unknown tier")
    check("malformed triple", {"target": [clone(rust_target="not a triple!!")]}, "malformed rust target")
    check("qemu without platform",
          {"target": [clone(id="q", platform_tag="manylinux_2_17_armv7l",
                            rust_target="armv7-unknown-linux-gnueabihf",
                            smoke="qemu", artifact="wheel-q",
                            arch_family="armv7")]},
          "qemu smoke needs qemu_platform")
    check("generic linux tag",
          {"target": [clone(platform_tag="linux_x86_64")]}, "generic linux_*")
    # Plan 268 Track A: baseline/policy separation.
    check("pypi as container selector",
          {"target": [clone(manylinux="pypi")]}, "reused as container")
    check("manylinux target with no baseline",
          {"target": [clone(manylinux="auto")]}, "no manylinux baseline")
    check("musllinux with manylinux baseline",
          {"target": [clone(id="m", platform_tag="musllinux_1_2_x86_64",
                            rust_target="x86_64-unknown-linux-musl",
                            manylinux="2_17", libc="musl",
                            artifact="wheel-m")]}, "manylinux baseline")
    check("glibc with musl baseline",
          {"target": [clone(manylinux="musllinux_1_2")]}, "musl baseline")
    check("stale maturin_compat",
          {"target": [dict(clone(), maturin_compat="pypi")]}, "stale key")
    # Plan 268 Track B: deferred routing.
    check("cross-built native smoke",
          {"target": [clone(id="x", platform_tag="manylinux_2_17_aarch64",
                            rust_target="aarch64-unknown-linux-gnu",
                            smoke="native", artifact="wheel-x",
                            arch_family="aarch64",
                            qualify_runner="ubuntu-24.04-arm")]},
          "cannot claim build-host native smoke")
    check("cross-arch native without qualifier",
          {"target": [clone(id="y", platform_tag="manylinux_2_17_aarch64",
                            rust_target="aarch64-unknown-linux-gnu",
                            smoke="native", artifact="wheel-y",
                            arch_family="aarch64")]},
          "cannot claim build-host native smoke")
    check("deferred without qualifier",
          {"target": [clone(id="z", platform_tag="manylinux_2_17_aarch64",
                            rust_target="aarch64-unknown-linux-gnu",
                            smoke="deferred-native", artifact="wheel-z",
                            arch_family="aarch64")]},
          "needs qualify_runner")
    check("deferred-native valid",
          {"target": [clone(id="d", platform_tag="manylinux_2_17_aarch64",
                            rust_target="aarch64-unknown-linux-gnu",
                            smoke="deferred-native", artifact="wheel-d",
                            arch_family="aarch64",
                            qualify_runner="ubuntu-24.04-arm")]}, None)
    check("deferred-container valid",
          {"target": [clone(id="e", platform_tag="musllinux_1_2_aarch64",
                            rust_target="aarch64-unknown-linux-musl",
                            manylinux="musllinux_1_2", libc="musl",
                            smoke="deferred-container", artifact="wheel-e",
                            arch_family="aarch64",
                            qualify_runner="ubuntu-24.04-arm")]}, None)

    # Real manifest must validate.
    real_errs = validate_matrix(DEFAULT_MATRIX)
    if real_errs:
        print(f"FAIL real manifest: {real_errs}")
        failures += 1
    else:
        print("OK real manifest validates")
    data = load_matrix(DEFAULT_MATRIX)
    tags = required_tags(data)
    if "musllinux_1_2_armv7l" not in tags:
        print("FAIL real manifest missing musllinux_1_2_armv7l")
        failures += 1
    else:
        print("OK real manifest contains musllinux_1_2_armv7l")
    if len(tags) != 10:
        print(f"FAIL required platform tag count != 10: {tags}")
        failures += 1
    else:
        print("OK required platform tag count is 10")
    pairs = required_pairs(data)
    if len(pairs) != 13:
        print(f"FAIL required target count != 13 (10 GIL + 3 ft): {pairs}")
        failures += 1
    else:
        print("OK required target count is 13 (10 GIL + 3 free-threaded)")
    ft_ids = {
        t["id"] for t in data.get("target", [])
        if t.get("abi_family") == "cp314t"
    }
    expected_ft = {
        "linux-x86_64-glibc-ft", "macos-arm64-ft", "windows-x86_64-ft",
    }
    if ft_ids != expected_ft:
        print(f"FAIL free-threaded target set != expected: {sorted(ft_ids)}")
        failures += 1
    else:
        print("OK free-threaded family covers exactly the three declared targets")
    # Plan 268: emitted matrix must carry the split controls and must never
    # emit `pypi` as the container selector.
    emitted = emit_gha_matrix(data)
    if any(e.get("manylinux") == "pypi" for e in emitted):
        print("FAIL emitted matrix reuses pypi as manylinux selector")
        failures += 1
    else:
        print("OK emitted matrix keeps pypi out of manylinux selector")
    if any("compatibility" not in e for e in emitted):
        print("FAIL emitted matrix missing compatibility")
        failures += 1
    else:
        print("OK emitted matrix carries compatibility")
    if any(
        not e.get("abi_family") or not e.get("interpreter")
        or not e.get("setup_python")
        for e in emitted
    ):
        print("FAIL emitted matrix missing family/interpreter fields")
        failures += 1
    else:
        print("OK emitted matrix carries abi_family/interpreter/setup_python")
    # Deferred targets must not request build-host native smoke.
    by_id = {e["id"]: e for e in emitted}
    for did in ("linux-aarch64-glibc", "windows-arm64", "linux-aarch64-musl"):
        e = by_id.get(did)
        if e is None:
            print(f"FAIL emitted matrix missing {did}")
            failures += 1
        elif e.get("smoke") == "native":
            print(f"FAIL {did} still claims build-host native smoke")
            failures += 1
        else:
            print(f"OK {did} deferred smoke={e.get('smoke')}")
    # manylinux baselines actually target the declared family.
    for e in emitted:
        tag = e.get("platform_tag", "")
        base_sel = e.get("manylinux", "")
        if tag.startswith("manylinux_2_17_") and base_sel != "2_17":
            print(f"FAIL {e['id']}: tag {tag} vs baseline {base_sel}")
            failures += 1
    print("OK emitted manylinux baselines match tags")

    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command",
                        choices=["validate", "emit-matrix", "expected-tags", "self-test"])
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--tier", default="required",
                        choices=["required", "candidate", "deferred", "all"])
    args = parser.parse_args(argv)

    if args.command == "self-test":
        return cmd_self_test()

    errors = validate_matrix(args.matrix)
    if errors:
        print("Wheel matrix INVALID:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    data = load_matrix(args.matrix)
    if args.command == "validate":
        pairs = required_pairs(data)
        tags = required_tags(data)
        print(
            f"Wheel matrix valid: {len(pairs)} required targets on "
            f"{len(tags)} platform tags"
        )
        for tag, family in pairs:
            print(f"  {family:7s} {tag}")
        return 0
    if args.command == "emit-matrix":
        print(json.dumps(emit_gha_matrix(data)))
        return 0
    if args.command == "expected-tags":
        if args.tier == "all":
            tags = declared_tags(data)
        else:
            tags = sorted(
                t["platform_tag"] for t in data.get("target", [])
                if t.get("tier") == args.tier
            )
        print("\n".join(tags))
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
