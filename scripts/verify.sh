#!/usr/bin/env bash
set -euo pipefail

# verify.sh — Local verification for eggserve
#
# Modes:
#   fast   Routine dev: format, clippy (lib/bins/tests), workspace tests
#   full   Pre-release: fast + TLS feature tests, Python wheel, package dry-run
#   deep   Expensive suites: full + corpus replay, fault injection, races, TLS abuse

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON="${PYTHON:-python3.14}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()    { printf "\033[0;34m▸\033[0m %s\n" "$*"; }
success() { printf "\033[0;32m✓\033[0m %s\n" "$*"; }
fail()    { printf "\033[0;31m✗\033[0m %s\n" "$*"; }
warn()    { printf "\033[0;33m⚠\033[0m %s\n" "$*"; }

header() {
  printf "\n\033[1m\033[0;35m━━━ %s ━━━\033[0m\n\n" "$*"
}

die() {
  printf "\033[0;31mFATAL:\033[0m %s\n" "$*" >&2
  exit 1
}

# Run a command, stream output, stop on first failure.
run() {
  info "$*"
  if "$@"; then
    success "$*"
  else
    local rc=$?
    fail "$* (exit $rc)"
    return "$rc"
  fi
}

command_exists() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------

cmd_fast() {
  header "Fast validation"
  run python3 "$REPO_ROOT/scripts/verify-conformance-matrix.py"
  run cargo fmt --all -- --check
  run cargo clippy --workspace --lib --bins --tests -- -D warnings
  run cargo test --workspace
}

cmd_full() {
  header "Full validation"
  cmd_fast

  header "TLS feature tests"
  run cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
  run cargo test -p eggserve-bin --features tls

  header "Executable examples"
  run cargo check -p eggserve-core --examples
  run cargo build -p eggserve-core --examples
  run bash "$SCRIPT_DIR/test-examples.sh"

  # Python wheel tests
  if command_exists "$PYTHON" && "$PYTHON" -m maturin --version >/dev/null 2>&1; then
    header "Python wheel tests"
    run env PYTHON="$PYTHON" bash "$SCRIPT_DIR/test-python-wheel.sh"
  else
    die "Python 3.14 and maturin are required for 'verify.sh full'.
Use 'verify.sh fast' for Rust-only development checks."
  fi

  # Package dry-run
  if [ -f "$SCRIPT_DIR/verify-cargo-packages.sh" ]; then
    header "Package dry-run"
    ALLOW_DIRTY=true run bash "$SCRIPT_DIR/verify-cargo-packages.sh" --mode all
  fi
}

cmd_deep() {
  header "Deep validation"
  cmd_full

  header "Expensive suites"

  # Corpus replay
  if [ -d "$REPO_ROOT/fuzz/corpus" ]; then
    run cargo test -p eggserve-core --test corpus_replay
  else
    info "No fuzz/corpus directory — skipping corpus replay"
  fi

  # Stateful fuzz replay
  run cargo test -p eggserve-core --test stateful_fuzz_replay

  # Fault injection
  run cargo test -p eggserve-core --test fault_injection

  # Filesystem race qualification
  run cargo test -p eggserve-core --test filesystem_race_qualification

  # TLS abuse (needs tls feature)
  run cargo test -p eggserve-bin --test tls_abuse --features tls

  # Proxy interop (needs Caddy/nginx)
  if command_exists caddy && command_exists nginx; then
    run bash "$REPO_ROOT/tests/proxy/caddy_interop.sh"
    run bash "$REPO_ROOT/tests/proxy/nginx_interop.sh"
    run bash "$REPO_ROOT/tests/proxy/desync_corpus.sh"
  else
    if [[ "${EGGSERVE_REQUIRE_PROXY:-0}" == "1" ]]; then
        die "Caddy/nginx required by EGGSERVE_REQUIRE_PROXY=1 but not in PATH"
    fi
    warn "Caddy/nginx not in PATH — proxy interop tests SKIPPED"
  fi
}

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

usage() {
  cat <<EOF
Usage: $0 <mode>

Modes:
  fast   Format, clippy, workspace tests
  full   Fast + TLS feature tests, Python wheel, package dry-run
  deep   Full + corpus replay, fuzz, fault injection, races, proxy interop

Examples:
  $0 fast    # routine dev check
  $0 full    # pre-merge / pre-release check
  $0 deep    # expensive suites (run manually)
EOF
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

mode="${1:-}"
case "$mode" in
  fast) cmd_fast ;;
  full) cmd_full ;;
  deep) cmd_deep ;;
  *) usage; exit 1 ;;
esac
