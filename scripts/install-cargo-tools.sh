#!/usr/bin/env bash
set -euo pipefail

# Keep these versions in one place. CI and release validation call this script
# instead of relying on tools preinstalled on a hosted runner.
readonly CARGO_AUDIT_VERSION="0.22.2"
readonly CARGO_DENY_VERSION="0.19.0"

cargo install cargo-audit --version "$CARGO_AUDIT_VERSION" --locked --force
cargo install cargo-deny --version "$CARGO_DENY_VERSION" --locked --force

tool_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH="$tool_bin:$PATH"
if [ -n "${GITHUB_PATH:-}" ]; then
  printf '%s\n' "$tool_bin" >> "$GITHUB_PATH"
fi

audit_version="$(cargo audit --version)"
deny_version="$(cargo deny --version)"

if [ "${audit_version##* }" != "$CARGO_AUDIT_VERSION" ]; then
  echo "cargo-audit version mismatch: expected $CARGO_AUDIT_VERSION, got $audit_version" >&2
  exit 1
fi

if [ "${deny_version##* }" != "$CARGO_DENY_VERSION" ]; then
  echo "cargo-deny version mismatch: expected $CARGO_DENY_VERSION, got $deny_version" >&2
  exit 1
fi

echo "$audit_version"
echo "$deny_version"
