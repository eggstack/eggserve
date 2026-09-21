#!/usr/bin/env python3
"""Run the installed-wheel Python typing fixture with mypy."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python", required=True, help="installed-wheel interpreter")
    parser.add_argument("fixture", type=Path)
    args = parser.parse_args()
    return subprocess.run(
        [
            args.python,
            "-m",
            "mypy",
            "--strict",
            "--no-incremental",
            str(args.fixture),
        ],
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
