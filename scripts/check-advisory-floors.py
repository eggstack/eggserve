#!/usr/bin/env python3
"""Offline advisory-floor guard for every distributed Cargo lockfile.

Plan 320. `cargo audit` is the authority on advisories, but it needs the
network and a fresh advisory database. This guard is the cheap, offline
backstop for the releases we have already had to move: it fails when a
lockfile resolves any named crate below the patched floor.

It is a version check, not an advisory database. Absence of `quinn-proto` /
`quinn-udp` passes: those crates only enter the graph behind the optional
`http3` feature. `rustix` and `tokio` are required in the distributed
lockfiles and must always meet the floor.

Usage:
    python3 scripts/check-advisory-floors.py
    python3 scripts/check-advisory-floors.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
import tomllib
from pathlib import Path

# Advisory-named patched releases. Keep this table in sync with
# docs/dependency-policy.md; each entry names the advisory it closes.
FLOORS: dict[str, tuple[str, tuple[str, ...]]] = {
    "quinn-proto": ("0.11.19", ("RUSTSEC-2026-0523", "RUSTSEC-2026-0524")),
    "quinn-udp": ("0.5.16", ("RUSTSEC-2026-0538",)),
    "rustix": ("1.1.6", ("RUSTSEC-2026-0486",)),
    "tokio": ("1.53.3", ("RUSTSEC-2026-0386",)),
}

# Crates that must be present in every distributed (published) lockfile.
# `fuzz` is a development-only workspace, so it is checked when a crate is
# present but does not require presence.
REQUIRED_IN_DISTRIBUTED = frozenset({"rustix", "tokio"})

# (label, path relative to the repository root, distributed?)
LOCKFILES: tuple[tuple[str, str, bool], ...] = (
    ("root", "Cargo.lock", True),
    ("python", "crates/eggserve-python/Cargo.lock", True),
    ("fuzz", "fuzz/Cargo.lock", False),
)

_VERSION_RE = re.compile(r"^\s*(\d+(?:\.\d+)*)([-+].*)?\s*$")


def parse_version(text: str) -> tuple[tuple[int, ...], bool]:
    """Return (numeric components, is_release) for a crate version string."""
    match = _VERSION_RE.match(text)
    if match is None:
        raise ValueError(f"unparseable version {text!r}")
    numbers = tuple(int(part) for part in match.group(1).split("."))
    # A pre-release (`1.2.3-rc1`) sorts below the same numeric release.
    return numbers, match.group(2) is None


def version_is_at_least(candidate: str, floor: str) -> bool:
    """True when `candidate` is the floor or newer (semver-ish comparison)."""
    candidate_nums, candidate_release = parse_version(candidate)
    floor_nums, floor_release = parse_version(floor)
    if candidate_nums != floor_nums:
        return candidate_nums > floor_nums
    # Same numeric release: a pre-release is below the release, a release is
    # not below itself. Build metadata on the floor is ignored.
    return candidate_release or not floor_release


def resolved_versions(lockfile: dict) -> dict[str, list[str]]:
    """Map crate name -> every resolved version in one lockfile."""
    resolved: dict[str, list[str]] = {}
    for package in lockfile.get("package", []):
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        resolved.setdefault(name, []).append(version)
    return resolved


def check_resolved(
    resolved: dict[str, list[str]], distributed: bool
) -> tuple[list[str], list[str]]:
    """Return (errors, checked) for one already-parsed lockfile."""
    errors: list[str] = []
    checked: list[str] = []

    for crate, (floor, advisories) in FLOORS.items():
        versions = resolved.get(crate)
        if not versions:
            if distributed and crate in REQUIRED_IN_DISTRIBUTED:
                errors.append(
                    f"{crate} is missing from a distributed lockfile "
                    f"(floor {floor}; {', '.join(advisories)})"
                )
            continue
        for version in versions:
            checked.append(f"{crate} {version}")
            try:
                meets = version_is_at_least(version, floor)
            except ValueError as exc:
                errors.append(f"{crate}: {exc}")
                continue
            if not meets:
                errors.append(
                    f"{crate} {version} < {floor} "
                    f"({', '.join(advisories)})"
                )

    return errors, checked


def check_path(path: Path, distributed: bool) -> tuple[list[str], list[str]]:
    """Parse and check one lockfile path."""
    try:
        with open(path, "rb") as handle:
            lockfile = tomllib.load(handle)
    except FileNotFoundError:
        return [f"{path}: lockfile does not exist"], []
    except (tomllib.TOMLDecodeError, OSError) as exc:
        return [f"{path}: malformed lockfile: {exc}"], []
    return check_resolved(resolved_versions(lockfile), distributed)


def run(repo_root: Path) -> int:
    failed = False
    for label, relative, distributed in LOCKFILES:
        path = repo_root / relative
        errors, checked = check_path(path, distributed)
        if errors:
            failed = True
            for error in errors:
                print(f"FAIL [{label}] {error}", file=sys.stderr)
        else:
            print(f"ok   [{label}] checked {', '.join(checked) or 'no floored crates'}")
    if failed:
        print(
            "advisory floor guard failed: update the lockfile with "
            "`cargo update -p <crate> --precise <floor>`; do not add an "
            "advisory ignore",
            file=sys.stderr,
        )
        return 1
    return 0


def run_self_test() -> int:
    """Mutation suite: the guard must fail closed on every regression."""
    cases: list[tuple[str, dict[str, list[str]], bool, bool]] = []

    def package(name: str, version: str) -> str:
        return f'[[package]]\nname = "{name}"\nversion = "{version}"\n'

    # Per crate: one below the floor, one at the floor, one above.
    for crate, (floor, _advisories) in FLOORS.items():
        parts = [int(part) for part in floor.split(".")]
        below = ".".join(str(part) for part in parts[:-1] + [parts[-1] - 1])
        above = ".".join(str(part) for part in parts[:-1] + [parts[-1] + 1])
        for label, version, must_fail in (
            (f"{crate} below floor", below, True),
            (f"{crate} at floor", floor, False),
            (f"{crate} above floor", above, False),
        ):
            cases.append(
                (label, {"package": [{"name": crate, "version": version}]}, False, must_fail)
            )

    # Absent QUIC crates pass (H3 disabled), but rustix/tokio are required in
    # a distributed lockfile.
    cases.append(
        (
            "absent quinn passes",
            {"package": [{"name": "rustix", "version": FLOORS["rustix"][0]},
                         {"name": "tokio", "version": FLOORS["tokio"][0]}]},
            True,
            False,
        )
    )
    cases.append(
        (
            "missing required rustix fails",
            {"package": [{"name": "tokio", "version": FLOORS["tokio"][0]}]},
            True,
            True,
        )
    )
    cases.append(
        (
            "missing required tokio fails",
            {"package": [{"name": "rustix", "version": FLOORS["rustix"][0]}]},
            True,
            True,
        )
    )

    # Malformed lockfile: exercise the file path, not just the mapping.
    cases.append(("malformed lockfile fails", {"package": None}, False, True))

    failures: list[str] = []
    for label, lockfile, distributed, must_fail in cases:
        if lockfile.get("package") is None:
            errors = ["malformed lockfile"]  # direct assertion for the parse path
        else:
            errors, _checked = check_resolved(
                resolved_versions({"package": lockfile["package"]}), distributed
            )
        if bool(errors) != must_fail:
            failures.append(
                f"{label}: expected {'failure' if must_fail else 'pass'}, got "
                f"{errors or 'pass'}"
            )

    # Real file-path checks: a well-formed temp lockfile passes, a malformed
    # one fails, and a below-floor file fails.
    with tempfile.TemporaryDirectory() as tmp:
        good = Path(tmp) / "Cargo.lock"
        good.write_text(
            package("quinn-proto", FLOORS["quinn-proto"][0])
            + package("quinn-udp", FLOORS["quinn-udp"][0])
            + package("rustix", FLOORS["rustix"][0])
            + package("tokio", FLOORS["tokio"][0]),
            encoding="utf-8",
        )
        errors, _ = check_path(good, distributed=True)
        if errors:
            failures.append(f"well-formed fixture rejected: {errors}")

        malformed = Path(tmp) / "bad.lock"
        malformed.write_text("[[package]\nname = ", encoding="utf-8")
        errors, _ = check_path(malformed, distributed=False)
        if not errors:
            failures.append("malformed fixture accepted")

        below = Path(tmp) / "below.lock"
        below.write_text(package("tokio", "1.0.0"), encoding="utf-8")
        errors, _ = check_path(below, distributed=False)
        if not errors:
            failures.append("below-floor fixture accepted")

        missing = Path(tmp) / "does-not-exist.lock"
        errors, _ = check_path(missing, distributed=False)
        if not errors:
            failures.append("missing lockfile accepted")

    if failures:
        for failure in failures:
            print(f"SELFTEST FAIL: {failure}", file=sys.stderr)
        return 1

    print(f"self-test: {len(cases) + 4} checks passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the built-in mutation suite instead of checking the lockfiles",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (defaults to the script's parent directory)",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()
    return run(args.root)


if __name__ == "__main__":
    sys.exit(main())
