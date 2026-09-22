#!/usr/bin/env python3
"""Canonical wheel-matrix authority tools (Plan 265).

Usage:
    python3 scripts/wheel-matrix.py validate [--matrix release/wheel-matrix.toml]
    python3 scripts/wheel-matrix.py emit-matrix [--matrix ...]  # GitHub Actions JSON
    python3 scripts/wheel-matrix.py expected-tags [--matrix ...] [--tier required]
    python3 scripts/wheel-matrix.py self-test

The manifest is the single authority for release wheel targets. The release
workflow and `check-release-wheel-set.py` consume it; no second manually
maintained platform list may remain.
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
VALID_SMOKES = {"native", "qemu", "post-publish-only", "container"}
VALID_LIBCS = {"glibc", "musl", "darwin", "msvc"}

RUST_TARGET_RE = re.compile(r"^[a-z0-9_]+-[a-z0-9_]+-[a-z0-9_]+(-[a-z0-9_]+)?$")
PLATFORM_TAG_RE = re.compile(r"^[A-Za-z0-9_]+$")


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
    seen_tags: dict[str, str] = {}
    seen_rust: dict[str, str] = {}
    for i, t in enumerate(targets):
        where = f"target[{i}]"
        if not isinstance(t, dict):
            errors.append(f"{where}: not a table")
            continue
        tid = t.get("id", f"<missing #{i}>")
        for key in (
            "id", "name", "rust_target", "platform_tag", "runner",
            "maturin_compat", "build", "smoke", "tier",
            "arch_family", "libc", "artifact",
        ):
            if key not in t:
                errors.append(f"{where} ({tid}): missing key {key!r}")
        tier = t.get("tier")
        if tier not in VALID_TIERS:
            errors.append(f"{where} ({tid}): unknown tier {tier!r}")
        if t.get("build") not in VALID_BUILDS:
            errors.append(f"{where} ({tid}): unknown build {t.get('build')!r}")
        if t.get("smoke") not in VALID_SMOKES:
            errors.append(f"{where} ({tid}): unknown smoke {t.get('smoke')!r}")
        if t.get("libc") not in VALID_LIBCS:
            errors.append(f"{where} ({tid}): unknown libc {t.get('libc')!r}")
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
        # Impossible smoke strategies.
        if t.get("smoke") == "qemu" and not t.get("qemu_platform"):
            errors.append(f"{where} ({tid}): qemu smoke needs qemu_platform")
        if t.get("build") == "native" and t.get("smoke") == "qemu":
            errors.append(
                f"{where} ({tid}): native build with qemu smoke is inconsistent"
            )
        # Duplicate detection.
        if tid in seen_ids:
            errors.append(f"duplicate target id {tid!r}")
        seen_ids[tid] = i
        if tag and tag in seen_tags:
            errors.append(
                f"duplicate platform tag {tag!r} "
                f"({seen_tags[tag]!r} and {tid!r})"
            )
            seen_tags[tag] = tid
        else:
            seen_tags[tag] = tid
        if rust_target and rust_target in seen_rust:
            errors.append(
                f"duplicate rust target {rust_target!r} "
                f"({seen_rust[rust_target]!r} and {tid!r})"
            )
        else:
            seen_rust[rust_target] = tid

    required = [t for t in targets if isinstance(t, dict) and t.get("tier") == "required"]
    if not required:
        errors.append("no required targets declared")
    return errors


def required_tags(data: dict) -> list[str]:
    return sorted(
        t["platform_tag"] for t in data.get("target", [])
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
            "manylinux": t["maturin_compat"],
            "artifact": t["artifact"],
            "platform_tag": t["platform_tag"],
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
        "maturin_compat": "pypi", "build": "cross-container", "smoke": "native",
        "tier": "required", "arch_family": "x86_64", "libc": "glibc",
        "artifact": "wheel-a",
    }

    def clone(**over: object) -> dict:
        d = dict(base)
        d.update(over)
        return d

    check("baseline valid", {"target": [clone()]}, None)
    other = clone(id="t2", platform_tag="manylinux_2_17_aarch64",
                  rust_target="aarch64-unknown-linux-gnu", artifact="wheel-b")
    check("two-target valid", {"target": [clone(), other]}, None)
    check("duplicate ids", {"target": [clone(), clone(id="t1", platform_tag="win_amd64",
          rust_target="x86_64-pc-windows-msvc", artifact="wheel-c")]}, "duplicate target id")
    check("duplicate tags", {"target": [clone(), clone(id="t2",
          rust_target="aarch64-unknown-linux-gnu", artifact="wheel-b")]}, "duplicate platform tag")
    check("unknown tier", {"target": [clone(tier="supported")]}, "unknown tier")
    check("malformed triple", {"target": [clone(rust_target="not a triple!!")]}, "malformed rust target")
    check("qemu without platform",
          {"target": [clone(id="q", platform_tag="manylinux_2_17_armv7l",
                            rust_target="armv7-unknown-linux-gnueabihf",
                            smoke="qemu", artifact="wheel-q")]},
          "qemu smoke needs qemu_platform")
    check("generic linux tag",
          {"target": [clone(platform_tag="linux_x86_64")]}, "generic linux_*")

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
        print(f"FAIL required tag count != 10: {tags}")
        failures += 1
    else:
        print("OK required tag count is 10")

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
        tags = required_tags(data)
        print(f"Wheel matrix valid: {len(tags)} required targets")
        for tag in tags:
            print(f"  {tag}")
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
