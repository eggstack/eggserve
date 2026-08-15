#!/usr/bin/env python3
"""Verify that a wheel contains the extension-backed CLI, not a second binary."""

from __future__ import annotations

import argparse
import sys
import zipfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="directory containing wheel files")
    args = parser.parse_args()

    wheels = sorted(args.directory.glob("*.whl"))
    if not wheels:
        print(f"no wheels found in {args.directory}", file=sys.stderr)
        return 1

    for wheel in wheels:
        with zipfile.ZipFile(wheel) as archive:
            forbidden = [
                name for name in archive.namelist() if name.startswith("eggserve/bin/eggserve")
            ]
        if forbidden:
            print(
                f"FAIL: {wheel.name} contains bundled executable: {forbidden}",
                file=sys.stderr,
            )
            return 1
        print(f"  {wheel.name}: no bundled server executable (good)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
