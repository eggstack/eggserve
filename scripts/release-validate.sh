#!/usr/bin/env bash
set -euo pipefail

# release-validate.sh — Unified local validation entry point for Plan 045
#
# Modes:
#   fast                Routine development: format, clippy, workspace tests, python source tests
#   full                Pre-release: everything in fast + TLS, client, wire, corpus, supply chain
#   gate <gate-id>      Run a single gate by looking up its command from criteria.toml
#   list                List all gates defined in criteria.toml
#   explain <gate-id>   Explain a single gate
#   check-generated     Verify generated files are clean (checklists, artifacts)
#
# Safety:
#   - Never publishes or requires production registry credentials
#   - Displays dirty-tree state (git status --porcelain)
#   - Preserves command exit codes
#   - Prints a summary with pass/fail/skip counts

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly CRITERIA_FILE="$REPO_ROOT/release/criteria.toml"
readonly CRITERIA_PY="$SCRIPT_DIR/release_criteria.py"

# ---------------------------------------------------------------------------
# Colors (disabled when not a TTY)
# ---------------------------------------------------------------------------

if [ -t 1 ]; then
  readonly RED='\033[0;31m'
  readonly GREEN='\033[0;32m'
  readonly YELLOW='\033[0;33m'
  readonly BLUE='\033[0;34m'
  readonly MAGENTA='\033[0;35m'
  readonly CYAN='\033[0;36m'
  readonly BOLD='\033[1m'
  readonly DIM='\033[2m'
  readonly RESET='\033[0m'
else
  readonly RED=''
  readonly GREEN=''
  readonly YELLOW=''
  readonly BLUE=''
  readonly MAGENTA=''
  readonly CYAN=''
  readonly BOLD=''
  readonly DIM=''
  readonly RESET=''
fi

# ---------------------------------------------------------------------------
# Counters
# ---------------------------------------------------------------------------

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
TOTAL_COUNT=0

# Track first failure for exit code
FIRST_FAILURE=""

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()    { printf "${BLUE}▸${RESET} %s\n" "$*"; }
success() { printf "${GREEN}✓${RESET} %s\n" "$*"; }
warn()    { printf "${YELLOW}⚠${RESET} %s\n" "$*"; }
fail()    { printf "${RED}✗${RESET} %s\n" "$*"; }
skip()    { printf "${CYAN}⊘${RESET} %s\n" "$*"; }

header() {
  printf "\n${BOLD}${MAGENTA}━━━ %s ━━━${RESET}\n\n" "$*"
}

die() {
  printf "${RED}FATAL:${RESET} %s\n" "$*" >&2
  exit 1
}

# Run a gate command and record pass/fail/skip.
# Usage: run_gate <gate-id> <title> <command>
run_gate() {
  local gate_id="$1"
  local title="$2"
  local command="$3"

  TOTAL_COUNT=$((TOTAL_COUNT + 1))

  printf "${BOLD}[%s]${RESET} %s\n" "$gate_id" "$title"
  printf "${DIM}  $ %s${RESET}\n" "$command"

  local start_time
  start_time="$(date +%s)"

  local exit_code=0
  if eval "$command"; then
    local end_time
    end_time="$(date +%s)"
    local duration=$((end_time - start_time))
    success "passed (${duration}s)"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    exit_code=$?
    local end_time
    end_time="$(date +%s)"
    local duration=$((end_time - start_time))
    fail "FAILED (exit ${exit_code}, ${duration}s)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    if [ -z "$FIRST_FAILURE" ]; then
      FIRST_FAILURE="$gate_id"
    fi
  fi
  printf "\n"
  return 0
}

# Run a command directly (not as a gate) and propagate exit code.
# Usage: run_step <description> <command...>
run_step() {
  local description="$1"
  shift

  info "$description"
  local exit_code=0
  if "$@"; then
    success "$description"
  else
    exit_code=$?
    fail "$description (exit ${exit_code})"
    return "$exit_code"
  fi
}

# Check if a command exists.
command_exists() {
  command -v "$1" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# Gate lookup: extract command from criteria.toml via Python
# ---------------------------------------------------------------------------

lookup_gate_command() {
  local gate_id="$1"
  python3 -c "
import sys
try:
    import tomllib
except ImportError:
    import tomli as tomllib

with open('$CRITERIA_FILE', 'rb') as f:
    data = tomllib.load(f)

for gate in data.get('gate', []):
    if gate['id'] == '$gate_id':
        cmd = gate.get('command')
        if cmd:
            print(cmd)
        else:
            print('NO_COMMAND', file=sys.stderr)
            sys.exit(1)
        sys.exit(0)

print('GATE_NOT_FOUND', file=sys.stderr)
sys.exit(1)
" 2>/dev/null
}

lookup_gate_title() {
  local gate_id="$1"
  python3 -c "
import sys
try:
    import tomllib
except ImportError:
    import tomli as tomllib

with open('$CRITERIA_FILE', 'rb') as f:
    data = tomllib.load(f)

for gate in data.get('gate', []):
    if gate['id'] == '$gate_id':
        print(gate.get('title', gate['id']))
        sys.exit(0)

print('GATE_NOT_FOUND', file=sys.stderr)
sys.exit(1)
" 2>/dev/null
}

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

preflight() {
  # Verify we're in a git repo
  if ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    die "Not inside a git repository"
  fi

  # Verify criteria.toml exists
  if [ ! -f "$CRITERIA_FILE" ]; then
    die "Criteria file not found: $CRITERIA_FILE"
  fi

  # Verify Python is available
  if ! command_exists python3; then
    die "python3 is required but not found in PATH"
  fi

  # Validate criteria.toml structure
  info "Validating criteria.toml"
  if ! python3 "$CRITERIA_PY" validate "$CRITERIA_FILE" >/dev/null 2>&1; then
    die "criteria.toml validation failed — fix errors before running gates"
  fi
  success "criteria.toml is valid"

  # Display dirty-tree state
  local dirty
  dirty="$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null || true)"
  if [ -n "$dirty" ]; then
    warn "Dirty working tree — local runs are not release evidence"
    printf "${DIM}$(echo "$dirty" | head -20)${RESET}\n"
    local line_count
    line_count="$(echo "$dirty" | wc -l | tr -d ' ')"
    if [ "$line_count" -gt 20 ]; then
      printf "${DIM}  ... and %d more files${RESET}\n" "$((line_count - 20))"
    fi
  else
    success "Clean working tree"
  fi
  printf "\n"
}

# ---------------------------------------------------------------------------
# Check-generated: verify generated files are clean
# ---------------------------------------------------------------------------

cmd_check_generated() {
  header "Generated file cleanliness"

  local failures=0

  # Check release checklist
  info "Checking docs/release-checklist.md"
  if python3 "$CRITERIA_PY" generate-checklist --check \
    --checklist-output "$REPO_ROOT/docs/release-checklist.md" \
    --criteria "$CRITERIA_FILE" 2>/dev/null; then
    success "release-checklist.md is up to date"
  else
    fail "release-checklist.md is stale — regenerate with: python scripts/release_criteria.py generate-checklist --check"
    failures=$((failures + 1))
  fi

  # Check Cargo.lock is not dirty after build
  info "Checking Cargo.lock freshness"
  if git -C "$REPO_ROOT" diff --name-only -- Cargo.lock 2>/dev/null | grep -q .; then
    fail "Cargo.lock has uncommitted changes"
    failures=$((failures + 1))
  else
    success "Cargo.lock is clean"
  fi

  # Check formatting hasn't drifted
  info "Checking rustfmt"
  if cargo fmt --all -- --check 2>/dev/null; then
    success "formatting is clean"
  else
    fail "formatting has drifted"
    failures=$((failures + 1))
  fi

  if [ "$failures" -gt 0 ]; then
    return 1
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Mode: fast
# ---------------------------------------------------------------------------

cmd_fast() {
  header "Fast validation (routine development)"

  # 1. Criteria validation (already done in preflight, but explicit here)
  run_gate "rust.format" "Rust formatting check" \
    "cargo fmt --all -- --check"

  run_gate "rust.clippy" "Workspace clippy lint" \
    "cargo clippy --workspace --all-targets -- -D warnings"

  run_gate "rust.test" "Workspace unit tests" \
    "cargo test --workspace"

  # Python source-only unit tests (no wheel needed)
  info "Python source-only unit tests"
  run_gate "python.unit-tests" "Python source-only unit tests" \
    "cd '$REPO_ROOT/crates/eggserve-python' && PYTHONPATH=python python3 -m unittest discover -s python -p 'test_*.py' -v"

  # Generated file cleanliness
  run_gate "check-generated" "Generated file cleanliness" \
    "bash '$SCRIPT_DIR/release-validate.sh' check-generated"
}

# ---------------------------------------------------------------------------
# Mode: full
# ---------------------------------------------------------------------------

cmd_full() {
  header "Full validation (pre-release)"

  # === Fast mode gates ===
  info "Running fast-mode gates first..."

  run_gate "rust.format" "Rust formatting check" \
    "cargo fmt --all -- --check"

  run_gate "rust.clippy" "Workspace clippy lint" \
    "cargo clippy --workspace --all-targets -- -D warnings"

  run_gate "rust.test" "Workspace unit tests" \
    "cargo test --workspace"

  # Python source-only tests
  run_gate "python.unit-tests" "Python source-only unit tests" \
    "cd '$REPO_ROOT/crates/eggserve-python' && PYTHONPATH=python python3 -m unittest discover -s python -p 'test_*.py' -v"

  # === Extended gates ===
  header "Extended Rust gates"

  run_gate "rust.doctest" "Rust doc tests" \
    "cargo test --workspace --doc"

  # TLS clippy + tests (server)
  run_gate "rust.test.server-tls" "Server TLS feature tests (clippy + test)" \
    "cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings && cargo test -p eggserve-bin --features tls"

  # Client feature tests
  run_gate "rust.test.client" "Client feature tests" \
    "cargo test -p eggserve-core --features client"

  # HTTP/filesystem correctness
  header "HTTP/filesystem correctness"

  run_gate "http.raw-wire" "Raw HTTP/1.x wire correctness tests" \
    "cargo test -p eggserve-core --test http_wire_correctness"

  run_gate "http.primitives-integration" "HTTP primitives integration tests" \
    "cargo test -p eggserve-core --test http_primitives_integration"

  run_gate "http.production-path" "Production path tests" \
    "cargo test -p eggserve-bin --test production_path"

  run_gate "filesystem.corpus-replay" "Fuzz corpus replay" \
    "cargo test -p eggserve-core --test corpus_replay && cargo test -p eggserve-core --test corpus_replay --features client"

  # Supply chain
  header "Supply chain gates"

  run_gate "supply-chain.audit" "cargo-audit vulnerability check" \
    "bash '$SCRIPT_DIR/install-cargo-tools.sh' && cargo audit"

  run_gate "supply-chain.deny" "cargo-deny license/policy check" \
    "cargo deny check"

  # Package verification
  header "Package verification"

  run_gate "package.core+bin" "Package/publish dry-run (core + bin)" \
    "bash '$SCRIPT_DIR/verify-cargo-packages.sh'"

  # Generated file cleanliness
  run_gate "check-generated" "Generated file cleanliness" \
    "bash '$SCRIPT_DIR/release-validate.sh' check-generated"
}

# ---------------------------------------------------------------------------
# Mode: gate <gate-id>
# ---------------------------------------------------------------------------

cmd_gate() {
  local gate_id="$1"

  if [ -z "$gate_id" ]; then
    die "Usage: $0 gate <gate-id>"
  fi

  header "Single gate: $gate_id"

  # Look up command from criteria.toml
  local command
  if ! command="$(lookup_gate_command "$gate_id")"; then
    die "Gate '$gate_id' not found in $CRITERIA_FILE"
  fi

  local title
  title="$(lookup_gate_title "$gate_id")"

  run_gate "$gate_id" "$title" "$command"
}

# ---------------------------------------------------------------------------
# Mode: list
# ---------------------------------------------------------------------------

cmd_list() {
  header "All gates"

  python3 "$CRITERIA_PY" list --criteria "$CRITERIA_FILE" --format text
}

# ---------------------------------------------------------------------------
# Mode: explain <gate-id>
# ---------------------------------------------------------------------------

cmd_explain() {
  local gate_id="$1"

  if [ -z "$gate_id" ]; then
    die "Usage: $0 explain <gate-id>"
  fi

  header "Gate details: $gate_id"

  python3 "$CRITERIA_PY" explain "$gate_id" --criteria "$CRITERIA_FILE" --format text
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

print_summary() {
  printf "\n"
  header "Summary"

  local total=$((PASS_COUNT + FAIL_COUNT + SKIP_COUNT))
  printf "  ${GREEN}✓ passed:${RESET}  %d\n" "$PASS_COUNT"
  printf "  ${RED}✗ failed:${RESET}  %d\n" "$FAIL_COUNT"
  printf "  ${CYAN}⊘ skipped:${RESET} %d\n" "$SKIP_COUNT"
  printf "  ─────────────\n"
  printf "  ${BOLD}total:%d${RESET}\n" "$total"
  printf "\n"

  if [ "$FAIL_COUNT" -gt 0 ]; then
    printf "${RED}${BOLD}RESULT: FAILED${RESET}\n"
    if [ -n "$FIRST_FAILURE" ]; then
      printf "${DIM}  First failure: %s${RESET}\n" "$FIRST_FAILURE"
    fi
    printf "\n"
    return 1
  else
    printf "${GREEN}${BOLD}RESULT: PASSED${RESET}\n\n"
    return 0
  fi
}

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

usage() {
  cat <<EOF
${BOLD}release-validate.sh${RESET} — Unified local validation for eggserve

${BOLD}Usage:${RESET}
  $0 fast                 Fast mode for routine development
  $0 full                 Full mode for pre-release validation
  $0 gate <gate-id>       Run a single gate from criteria.toml
  $0 list                 List all gates
  $0 explain <gate-id>    Explain a single gate
  $0 check-generated      Verify generated files are clean
  $0 help                 Show this help

${BOLD}Modes:${RESET}
  ${GREEN}fast${RESET}             Format, clippy, workspace tests, python source tests, generated files
  ${GREEN}full${RESET}             Everything in fast + TLS, client, wire, corpus, supply chain, packages
  ${GREEN}gate <id>${RESET}         Look up gate command from criteria.toml and run it
  ${GREEN}list${RESET}             Display all gates from criteria.toml
  ${GREEN}explain <id>${RESET}      Show full details for a specific gate
  ${GREEN}check-generated${RESET}   Verify checklists and artifacts are up to date

${BOLD}Safety:${RESET}
  - Never publishes or requires production registry credentials
  - Dirty-tree state is displayed before execution
  - Exit code reflects first gate failure

${BOLD}Examples:${RESET}
  $0 fast                   # routine dev check
  $0 full                   # pre-release validation
  $0 gate rust.format       # run just the format check
  $0 gate http.raw-wire     # run just the wire tests
  $0 explain rust.clippy    # show details for a gate
EOF
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  local mode="${1:-help}"
  shift || true

  case "$mode" in
    fast)
      preflight
      cmd_fast
      print_summary
      ;;
    full)
      preflight
      cmd_full
      print_summary
      ;;
    gate)
      preflight
      local gate_id="${1:-}"
      cmd_gate "$gate_id"
      print_summary
      ;;
    list)
      preflight
      cmd_list
      ;;
    explain)
      preflight
      local gate_id="${1:-}"
      cmd_explain "$gate_id"
      ;;
    check-generated)
      cmd_check_generated
      ;;
    help|--help|-h)
      usage
      exit 0
      ;;
    *)
      printf "${RED}Unknown mode:${RESET} %s\n\n" "$mode" >&2
      usage >&2
      exit 1
      ;;
  esac
}

main "$@"
