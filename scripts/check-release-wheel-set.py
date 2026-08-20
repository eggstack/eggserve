#!/usr/bin/env python3
"""Validate a complete set of release wheels for the eggserve project.

Usage:
    python check-release-wheel-set.py <directory> --version <expected>
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import re
import sys
import zipfile
from pathlib import Path

REQUIRED_TARGETS = {
    "manylinux_2_17_x86_64",
    "manylinux_2_17_aarch64",
    "manylinux_2_17_armv7l",
    "musllinux_1_2_x86_64",
    "musllinux_1_2_aarch64",
    "macosx_11_0_arm64",
    "macosx_11_0_x86_64",
    "win_amd64",
    "win_arm64",
}


def verify_record(archive: zipfile.ZipFile) -> list[str]:
    """Verify RECORD file integrity using sha256 hashes. Returns error messages."""
    errors = []
    record_names = [n for n in archive.namelist() if n.endswith("/RECORD") or n == "RECORD"]
    if not record_names:
        return ["no RECORD file in wheel"]

    raw = archive.read(record_names[0]).decode("utf-8", errors="replace")
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split(",")
        if len(parts) != 3:
            continue
        member, hash_digest, size = parts
        # RECORD entry for RECORD itself has empty hash
        if member == record_names[0] or not hash_digest:
            continue
        # The hash format is sha256=<urlsafe-base64-no-padding>
        # per PEP 376 / wheel RECORD spec.
        if not hash_digest.startswith("sha256="):
            errors.append(f"unexpected hash algorithm for {member}: {hash_digest}")
            continue
        expected_b64 = hash_digest[len("sha256="):]
        try:
            data = archive.read(member)
        except KeyError:
            errors.append(f"RECORD lists member '{member}' not found in archive")
            continue
        actual_b64 = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        if actual_b64 != expected_b64:
            errors.append(
                f"sha256 mismatch for {member}: RECORD={expected_b64[:16]}... "
                f"actual={actual_b64[:16]}..."
            )
    return errors


WHEEL_RE = re.compile(
    r"^(?P<distribution>[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)"
    r"-(?P<version>[A-Za-z0-9][A-Za-z0-9.!.post]*[A-Za-z0-9])"
    r"(?:-(?P<build>\d[A-Za-z0-9.]*))?"
    r"-(?P<python>[A-Za-z0-9_.]+)"
    r"-(?P<abi>[A-Za-z0-9_.]+)"
    r"-(?P<platform>[A-Za-z0-9_.]+)"
    r"\.whl$"
)

MANYLINUX_RE = re.compile(r"^manylinux\d+_\d+_(.*)")
MUSLLINUX_RE = re.compile(r"^musllinux\d+_\d+_(.*)")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="directory containing wheel files")
    parser.add_argument("--version", required=True, help="expected version string")
    return parser.parse_args()


def parse_platform_tags(tag_string: str) -> list[str]:
    """Wheel platform tags are dot-separated."""
    return tag_string.split(".")


def format_targets(targets: set[str]) -> str:
    return "\n  ".join(sorted(targets))


def main() -> int:
    args = parse_args()
    directory: Path = args.directory
    expected_version: str = args.version

    wheels = sorted(directory.glob("*.whl"))
    if not wheels:
        print(f"FAIL: no wheels found in {directory}", file=sys.stderr)
        return 1

    failures = 0
    parsed = []
    seen_platforms: dict[str, str] = {}
    all_platforms: set[str] = set()

    for wheel in wheels:
        # --- Check 1: filename shape ---
        match = WHEEL_RE.match(wheel.name)
        if match is None:
            print(f"FAIL: {wheel.name}: filename does not match expected shape", file=sys.stderr)
            failures += 1
            continue

        distribution = match.group("distribution")
        version = match.group("version")
        build = match.group("build")
        python_tag = match.group("python")
        abi_tag = match.group("abi")
        platform_tag = match.group("platform")

        platforms = parse_platform_tags(platform_tag)

        parsed.append({
            "wheel": wheel,
            "distribution": distribution,
            "version": version,
            "build": build,
            "python_tag": python_tag,
            "abi_tag": abi_tag,
            "platform_tag": platform_tag,
            "platforms": platforms,
        })

    if failures:
        return 1

    for entry in parsed:
        wheel = entry["wheel"]
        distribution = entry["distribution"]
        version = entry["version"]
        python_tag = entry["python_tag"]
        abi_tag = entry["abi_tag"]
        platform_tag = entry["platform_tag"]
        platforms = entry["platforms"]

        print(f"\n--- {wheel.name} ---")

        # --- Check 2: project name ---
        if distribution != "eggserve":
            print(f"  FAIL: distribution '{distribution}' != expected 'eggserve'", file=sys.stderr)
            failures += 1

        # --- Check 3 & 4: METADATA validation ---
        with zipfile.ZipFile(wheel) as archive:
            namelist = archive.namelist()

            # Find and read METADATA
            metadata_names = [n for n in namelist if n.endswith("/METADATA") or n == "METADATA"]
            if not metadata_names:
                print(f"  FAIL: no METADATA file in wheel", file=sys.stderr)
                failures += 1
            else:
                try:
                    raw_metadata = archive.read(metadata_names[0]).decode("utf-8", errors="replace")
                    # configparser can't handle METADATA with multi-line
                    # descriptions (markdown body, backticks, etc.). Extract
                    # the scalar fields we care about with a simple line scan.
                    def _scalar(name: str) -> str | None:
                        for line in raw_metadata.splitlines():
                            if line.startswith(f"{name}:"):
                                return line.split(":", 1)[1].strip()
                        return None

                    meta_name = _scalar("Name")
                    meta_version = _scalar("Version")

                    if meta_name != "eggserve":
                        print(
                            f"  FAIL: METADATA Name '{meta_name}' != expected 'eggserve'",
                            file=sys.stderr,
                        )
                        failures += 1
                    else:
                        print(f"  OK: METADATA Name = 'eggserve'")

                    if meta_version != expected_version:
                        print(
                            f"  FAIL: METADATA Version '{meta_version}' != expected '{expected_version}'",
                            file=sys.stderr,
                        )
                        failures += 1
                    else:
                        print(f"  OK: METADATA Version = '{expected_version}'")

                except (KeyError, ValueError) as exc:
                    print(f"  FAIL: could not parse METADATA: {exc}", file=sys.stderr)
                    failures += 1

            # Read and validate WHEEL tags
            wheel_names = [n for n in namelist if n.endswith("/WHEEL") or n == "WHEEL"]
            if not wheel_names:
                print(f"  FAIL: no WHEEL file in wheel", file=sys.stderr)
                failures += 1
            else:
                raw_wheel = archive.read(wheel_names[0]).decode("utf-8", errors="replace")

                # WHEEL file can have multiple Tag lines; take the first
                # Actually, the Tag field format is: Tag: cp311-abi3-manylinux_2_17_x86_64
                # Multiple Tag lines each contain a full triple. We want to check
                # that each tag's abi component is abi3 and python starts with cp311.
                tag_lines = []
                for line in raw_wheel.splitlines():
                    if line.lower().startswith("tag:"):
                        tag_lines.append(line.split(":", 1)[1].strip())

                all_abi3 = True
                all_cp311 = True
                for tag in tag_lines:
                    parts = tag.split("-")
                    if len(parts) == 3:
                        tag_python, tag_abi, tag_platform = parts
                        if tag_abi != "abi3":
                            all_abi3 = False
                        if not tag_python.startswith("cp311"):
                            all_cp311 = False

                if not all_abi3:
                    print(
                        f"  FAIL: not all WHEEL Tag abi fields are abi3: {tag_lines}",
                        file=sys.stderr,
                    )
                    failures += 1
                else:
                    print(f"  OK: WHEEL Tag abi = abi3")

                if not all_cp311:
                    print(
                        f"  FAIL: not all WHEEL Tag python fields start with cp311: {tag_lines}",
                        file=sys.stderr,
                    )
                    failures += 1
                else:
                    print(f"  OK: WHEEL Tag python starts with cp311")

                # Check filename abi/python tags match WHEEL tags
                if abi_tag != "abi3":
                    print(
                        f"  FAIL: filename abi tag '{abi_tag}' != expected 'abi3'",
                        file=sys.stderr,
                    )
                    failures += 1

                if not python_tag.startswith("cp311"):
                    print(
                        f"  FAIL: filename python tag '{python_tag}' does not start with 'cp311'",
                        file=sys.stderr,
                    )
                    failures += 1

            # --- Check 5: Tier 1 target set ---
            all_platforms.update(platforms)

            # --- Check 6: no duplicate platform target ---
            for plat in platforms:
                if plat in seen_platforms:
                    print(
                        f"  FAIL: duplicate platform tag '{plat}' in {wheel.name} "
                        f"(first seen in {seen_platforms[plat]})",
                        file=sys.stderr,
                    )
                    failures += 1
                seen_platforms[plat] = wheel.name

            # --- Check 7: no generic linux_* ---
            for plat in platforms:
                if plat.startswith("linux_"):
                    print(f"  FAIL: generic linux_* platform tag '{plat}' (must be manylinux or musllinux)", file=sys.stderr)
                    failures += 1

            # --- Check 8: manylinux and musllinux families not conflated ---
            families_in_wheel = set()
            for plat in platforms:
                if MANYLINUX_RE.match(plat):
                    families_in_wheel.add("manylinux")
                elif MUSLLINUX_RE.match(plat):
                    families_in_wheel.add("musllinux")
            if "manylinux" in families_in_wheel and "musllinux" in families_in_wheel:
                print(f"  FAIL: wheel mixes manylinux and musllinux families", file=sys.stderr)
                failures += 1

            # --- Check 9: no bundled eggserve/bin/eggserve executable ---
            forbidden = [
                name for name in namelist if name.startswith("eggserve/bin/eggserve")
            ]
            if forbidden:
                print(f"  FAIL: contains bundled executable: {forbidden}", file=sys.stderr)
                failures += 1
            else:
                print(f"  OK: no bundled server executable")

            # --- Check 10: Python package/native extension members present ---
            has_python_pkg = any(
                n.startswith("eggserve/") and n.endswith(".py") for n in namelist
            )
            has_native_ext = any(
                n.startswith("eggserve/") and (
                    n.endswith(".so") or n.endswith(".pyd") or n.endswith(".dylib")
                )
                for n in namelist
            )
            if not has_python_pkg:
                print(f"  FAIL: no Python package members (eggserve/**/*.py)", file=sys.stderr)
                failures += 1
            else:
                print(f"  OK: Python package members present")
            if not has_native_ext:
                print(f"  FAIL: no native extension members (eggserve/**/*.so/.pyd/.dylib)", file=sys.stderr)
                failures += 1
            else:
                print(f"  OK: native extension members present")

            # --- RECORD integrity (hashlib sha256) ---
            record_errors = verify_record(archive)
            if record_errors:
                for err in record_errors:
                    print(f"  FAIL: RECORD: {err}", file=sys.stderr)
                failures += 1
            else:
                print(f"  OK: RECORD integrity verified (sha256)")

    # --- Post-loop checks ---
    print(f"\n=== Summary ===")
    print(f"Wheels found: {len(wheels)}")

    # --- Check 5 continued: required Tier 1 target set is exact ---
    missing = REQUIRED_TARGETS - all_platforms
    extra = all_platforms - REQUIRED_TARGETS
    if missing:
        print(f"FAIL: missing required Tier 1 targets:\n  {format_targets(missing)}", file=sys.stderr)
        failures += 1
    if extra:
        print(f"FAIL: unexpected extra platform targets:\n  {format_targets(extra)}", file=sys.stderr)
        failures += 1
    if not missing and not extra:
        print(f"OK: Tier 1 target set is exact ({len(REQUIRED_TARGETS)} targets)")

    # --- Summary of platform distribution ---
    print(f"Platform targets found: {len(all_platforms)}")
    for plat in sorted(all_platforms):
        print(f"  {plat}")

    if failures == 0:
        print(f"\nAll checks passed.")
        return 0
    else:
        print(f"\n{failures} check(s) FAILED.")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
