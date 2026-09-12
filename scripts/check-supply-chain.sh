#!/usr/bin/env bash
set -euo pipefail

# Check every Cargo.lock that can contribute to a distributed artifact. The
# Python extension is intentionally excluded from the root workspace, so it
# must be audited and policy-checked from its own manifest and lockfile.
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly PYTHON_CRATE="$REPO_ROOT/crates/eggserve-python"

command -v cargo-audit >/dev/null 2>&1 || {
    echo "cargo-audit not found; run scripts/install-cargo-tools.sh first" >&2
    exit 1
}
command -v cargo-deny >/dev/null 2>&1 || {
    echo "cargo-deny not found; run scripts/install-cargo-tools.sh first" >&2
    exit 1
}

echo "== root dependency advisory audit =="
(cd "$REPO_ROOT" && cargo audit --file Cargo.lock)

echo "== root dependency policy check =="
(cd "$REPO_ROOT" && cargo deny check)

echo "== Python wheel dependency advisory audit =="
(cd "$REPO_ROOT" && cargo audit --file crates/eggserve-python/Cargo.lock)

echo "== Python wheel dependency policy check =="
(cd "$PYTHON_CRATE" && cargo deny check --config ../../deny.toml)
