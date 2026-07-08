"""Locate and execute the packaged eggserve Rust binary."""

import os
import sys
import subprocess
from pathlib import Path


def _find_binary() -> str:
    """Find the eggserve binary bundled in this package."""
    package_dir = Path(__file__).resolve().parent.parent
    bin_dir = package_dir / "bin"
    if sys.platform == "win32":
        candidate = bin_dir / "eggserve.exe"
    else:
        candidate = bin_dir / "eggserve"
    if candidate.is_file():
        return str(candidate)

    bin_dir2 = package_dir.parent / "bin"
    if sys.platform == "win32":
        candidate2 = bin_dir2 / "eggserve.exe"
    else:
        candidate2 = bin_dir2 / "eggserve"
    if candidate2.is_file():
        return str(candidate2)

    for path_entry in os.environ.get("PATH", "").split(os.pathsep):
        candidate3 = Path(path_entry) / "eggserve"
        if candidate3.is_file():
            return str(candidate3)

    print("error: eggserve binary not found", file=sys.stderr)
    sys.exit(1)


def main() -> int:
    """Execute the eggserve binary with forwarded CLI arguments."""
    binary = _find_binary()
    argv = [binary] + sys.argv[1:]
    try:
        result = subprocess.run(argv)
        return result.returncode
    except KeyboardInterrupt:
        return 130
    except FileNotFoundError:
        print(f"error: failed to execute {binary}", file=sys.stderr)
        return 1
