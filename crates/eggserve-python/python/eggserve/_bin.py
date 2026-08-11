"""Execute the eggserve CLI via the native Rust extension."""

import sys

from eggserve._native import _run_cli


def main() -> int:
    """Execute the eggserve CLI with forwarded arguments.

    Returns an integer exit code suitable for ``sys.exit()``.
    """
    try:
        return _run_cli(sys.argv[1:])
    except KeyboardInterrupt:
        return 130
