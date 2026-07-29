"""Repository fixture resolver for tests outside the package source tree.

Derives the repository root from this file's known source layout and
provides paths to shared conformance corpora and other test fixtures.
"""

from __future__ import annotations

import os
from pathlib import Path

_REPO_ROOT: Path | None = None


def repo_root() -> Path:
    """Return the eggserve repository root path.

    Validates that the expected workspace Cargo.toml exists at the
    derived location. Raises RuntimeError if the checkout layout
    is unavailable.
    """
    global _REPO_ROOT
    if _REPO_ROOT is not None:
        return _REPO_ROOT

    # tests/_repo.py -> tests/ -> eggserve-python/ -> crates/ -> repo root
    root = Path(__file__).resolve().parents[3]
    workspace = root / "Cargo.toml"
    if not workspace.is_file():
        raise RuntimeError(f"eggserve repository root not found: {root}")
    _REPO_ROOT = root
    return root


def conformance_corpus() -> Path:
    """Return the path to conformance/corpus.json."""
    path = repo_root() / "conformance" / "corpus.json"
    if not path.is_file():
        raise RuntimeError(f"conformance corpus not found: {path}")
    return path


def body_conformance_corpus() -> Path:
    """Return the path to conformance/body_corpus.json."""
    path = repo_root() / "conformance" / "body_corpus.json"
    if not path.is_file():
        raise RuntimeError(f"body conformance corpus not found: {path}")
    return path
