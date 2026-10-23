#!/usr/bin/env bash
set -euo pipefail

# verify.sh — Local verification for eggserve
#
# Modes:
#   fast   Routine dev: format, clippy, workspace tests, Python crate check
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

# Plan 312: opt-in cross-target compile coverage for the confinement
# authority. The check is deliberately scoped to `eggserve-static`, the only
# crate with platform-specific production code (`#[cfg(windows)]` handle-relative
# confinement, `#[cfg(unix)]` descriptor-relative traversal). A whole-workspace
# cross-check would additionally need a target-native C toolchain for `ring`
# and `aws-lc-sys`, which a developer rarely has; the hosted CI job
# (`.github/workflows/ci.yml`, job `platform-compile`) covers the full
# workspace natively instead.
#
# Plan 317: the Windows confinement path is now *tested* on a hosted Windows
# runner (the `Windows confinement test lane` in `ci.yml`), not merely
# compiled there. A cross-compiled target still cannot *run* those tests, so
# the local proxy remains the compile + clippy cross-check below; the run
# itself only happens on a native Windows host.
#
# Enable with EGGSERVE_PLATFORM_CHECK=1. Any skip is printed, never silent.
cmd_platform_check() {
  header "Platform compile coverage"

  if [[ "${EGGSERVE_PLATFORM_CHECK:-0}" != "1" ]]; then
    info "Platform compile coverage SKIPPED (opt-in: set EGGSERVE_PLATFORM_CHECK=1)"
    return 0
  fi

  if ! command_exists rustup; then
    info "Platform compile coverage SKIPPED (rustup not available)"
    return 0
  fi

  local targets="${EGGSERVE_PLATFORM_TARGETS:-x86_64-pc-windows-gnu aarch64-apple-darwin}"
  local installed
  installed="$(rustup target list --installed 2>/dev/null || true)"
  if [[ -z "$installed" ]]; then
    info "Platform compile coverage SKIPPED (rustup target list unavailable)"
    return 0
  fi

  local target
  for target in $targets; do
    if ! grep -qx -- "$target" <<<"$installed"; then
      info "Platform compile coverage SKIPPED for $target (target std not installed: rustup target add $target)"
      continue
    fi
    run cargo check -p eggserve-static --lib --target "$target"
    # Plan 313: `clippy::undocumented_unsafe_blocks` only fires on code the
    # compilation unit actually compiles. The Windows FFI boundary lives in
    # `#[cfg(windows)]` code, so it is only linted when the Windows target is
    # compiled — which the Windows platform lane in CI does natively and this
    # cross-check does for the static crate.
    run cargo clippy -p eggserve-static --lib --target "$target" -- -D warnings
  done
}

# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------

cmd_fast() {
  header "Fast validation"
  run python3 "$REPO_ROOT/scripts/verify-conformance-matrix.py"
  run python3 "$REPO_ROOT/scripts/check-crate-topology.py"
  run python3 "$REPO_ROOT/scripts/check-python-release-metadata.py"
  run cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features http-interop
  run cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
  run cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features http-interop
  run cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features tower
  run cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
  run cargo test -p eggserve-server --no-default-features --features tower
  run cargo clippy -p eggserve-core --no-default-features --features tower --lib --tests -- -D warnings
  run cargo test -p eggserve-core --no-default-features --features tower
  run cargo test -p eggserve-core --no-default-features --features http-interop --lib
  run cargo fmt --all -- --check
  run cargo clippy --workspace --lib --bins --tests -- -D warnings
  run cargo test --workspace
  run cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
  run cargo test -p eggserve-core --features http2,tls
  run cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
  run cargo test -p eggserve-bin --features http2,tls
  run cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
  run cargo test -p eggserve-core --features http3,tls
  run cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
  run cargo test -p eggserve-bin --features http3,tls
  run cargo check --manifest-path "$REPO_ROOT/crates/eggserve-python/Cargo.toml" --locked
  # Plan 313: the excluded Python crate is outside the workspace, so
  # `cargo clippy --workspace` never sees it. It carries the Python-facing
  # surface, so it gets the same lint strictness as the workspace crates.
  run cargo clippy --manifest-path "$REPO_ROOT/crates/eggserve-python/Cargo.toml" --lib -- -D warnings
  # Plan 313: the `python-bindings-internal` feature compiles the Python-facing
  # bridge code inside eggserve-static and eggserve-core. Without a lane that
  # code path is never linted.
  run cargo clippy -p eggserve-static --features python-bindings-internal --lib --tests -- -D warnings
  run cargo clippy -p eggserve-core --features python-bindings-internal --lib --tests -- -D warnings
  # Plan 310: the fuzz crate is a separate workspace with its own lockfile, so
  # `cargo test --workspace` never covers it. Build it here so a target that
  # rots against the current API fails a routine local check.
  run cargo check --manifest-path "$REPO_ROOT/fuzz/Cargo.toml" --bins --locked
  run cargo clippy --manifest-path "$REPO_ROOT/fuzz/Cargo.toml" --bins -- -D warnings
  run cargo fmt --manifest-path "$REPO_ROOT/fuzz/Cargo.toml" --all -- --check
  cmd_platform_check
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
