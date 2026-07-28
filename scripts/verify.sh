#!/usr/bin/env bash
set -euo pipefail

# verify.sh — Local verification for eggserve (Plan 091)
#
# Modes:
#   fast   Routine dev: format, clippy, workspace tests
#   full   Pre-release: fast + TLS/client features, Python wheel, package dry-run
#   deep   Expensive suites: full + corpus replay, fuzz, fault injection, races

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()    { printf "\033[0;34m▸\033[0m %s\n" "$*"; }
success() { printf "\033[0;32m✓\033[0m %s\n" "$*"; }
fail()    { printf "\033[0;31m✗\033[0m %s\n" "$*"; }

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
  run cargo fmt --all -- --check
  run cargo clippy --workspace --all-targets -- -D warnings
  run cargo test --workspace
}

cmd_full() {
  header "Full validation"
  cmd_fast

  header "Feature tests"
  run cargo test -p eggserve-core --features client-tls
  run cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
  run cargo test -p eggserve-bin --features tls

  # Python wheel tests
  if command_exists python3 && command_exists maturin; then
    header "Python wheel tests"
    (
      cd "$REPO_ROOT"
      cargo build --release --locked -p eggserve-bin
      mkdir -p crates/eggserve-python/python/eggserve/bin
      cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
      chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
      cd crates/eggserve-python
      maturin build --release --interpreter python -o dist
      pip install --force-reinstall dist/*.whl
      PYTHONPATH="" python -m unittest discover -s python -p 'test_*.py' -v
    )
  else
    info "Python/maturin not available — skipping wheel tests"
  fi

  # Package dry-run
  if [ -f "$SCRIPT_DIR/verify-cargo-packages.sh" ]; then
    header "Package dry-run"
    run bash "$SCRIPT_DIR/verify-cargo-packages.sh" --mode core
    run bash "$SCRIPT_DIR/verify-cargo-packages.sh" --mode bin
  fi
}

cmd_deep() {
  header "Deep validation"
  cmd_full

  header "Expensive suites"

  # Corpus replay (needs client feature for some tests)
  if [ -d "$REPO_ROOT/fuzz/corpus" ]; then
    run cargo test -p eggserve-core --test corpus_replay
    run cargo test -p eggserve-core --test corpus_replay --features client
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
    info "Caddy/nginx not available — skipping proxy interop"
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
  full   Fast + TLS/client features, Python wheel, package dry-run
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
