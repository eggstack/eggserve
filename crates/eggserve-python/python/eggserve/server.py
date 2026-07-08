"""Server utilities for eggserve.

This module is intentionally thin. The Rust binary is the source of truth
for all serving logic. This module exists for future Python API expansion.
"""

from eggserve._bin import _find_binary, main

__all__ = ["main", "_find_binary"]
