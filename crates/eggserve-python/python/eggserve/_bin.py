"""Locate and execute the packaged eggserve Rust binary."""

import os
import sys
import subprocess
from pathlib import Path


def _find_binary() -> str:
    """Find the eggserve binary bundled in this package.

    Raises FileNotFoundError if the binary cannot be found.
    """
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

    raise FileNotFoundError(
        "eggserve binary not found; ensure the package is installed correctly"
    )


def main() -> int:
    """Execute the eggserve binary with forwarded CLI arguments."""
    try:
        binary = _find_binary()
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    argv = [binary] + sys.argv[1:]
    try:
        result = subprocess.run(argv)
        return result.returncode
    except KeyboardInterrupt:
        return 130
    except FileNotFoundError:
        print(f"error: failed to execute {binary}", file=sys.stderr)
        return 1
