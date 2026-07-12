#!/usr/bin/env bash
# Standalone packaging smoke-test runner for eggserve.
#
# Usage:
#   ./run_all.sh <wheel-path> [python-interpreter]
#
# Creates a fresh virtual environment, installs the wheel (no source-tree
# contamination), copies the test scripts to a temp directory, runs them,
# and reports results.
#
# Requirements:
#   - A built wheel passed as $1
#   - Python 3 (defaults to python3, or $2)

set -euo pipefail

WHEEL="${1:?Usage: $0 <wheel-path> [python-interpreter]}"
PY="${2:-python3}"

if [ ! -f "$WHEEL" ]; then
    echo "ERROR: Wheel not found: $WHEEL" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORK_DIR="$(mktemp -d)"

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

echo "=== eggserve packaging smoke tests ==="
echo "Wheel:    $WHEEL"
echo "Python:   $PY"
echo "Work dir: $WORK_DIR"
echo ""

# 1. Create fresh venv (no system-site-packages)
echo "--- Creating virtual environment ---"
"$PY" -m venv "$WORK_DIR/venv"
VENV_PY="$WORK_DIR/venv/bin/python"
VENV_PIP="$WORK_DIR/venv/bin/pip"

# 2. Install the wheel (no source-tree PYTHONPATH)
echo "--- Installing wheel ---"
unset PYTHONPATH
"$VENV_PIP" install --no-deps "$WHEEL" 2>&1

# 3. Verify installation
echo "--- Verifying installation ---"
"$VENV_PY" -c "import eggserve; print(f'eggserve {eggserve.__version__} loaded (NATIVE_AVAILABLE={eggserve.NATIVE_AVAILABLE})')"

# 4. Copy test scripts to work directory (isolation from source tree)
echo "--- Copying test scripts ---"
cp "$SCRIPT_DIR"/test_imports.py "$WORK_DIR/"
cp "$SCRIPT_DIR"/test_server_smoke.py "$WORK_DIR/"
cp "$SCRIPT_DIR"/test_client_smoke.py "$WORK_DIR/"
cp "$SCRIPT_DIR"/test_cli_smoke.py "$WORK_DIR/"

# 5. Run each test file from the work directory
echo ""
echo "--- Running smoke tests ---"
echo ""

TESTS=("test_imports.py" "test_server_smoke.py" "test_client_smoke.py" "test_cli_smoke.py")
PASSED=0
FAILED=0
FAILURES=()

for test_file in "${TESTS[@]}"; do
    echo "=== $test_file ==="
    # Run directly as a script from the work directory (each file has unittest.main())
    if cd "$WORK_DIR" && "$VENV_PY" "$test_file" -v 2>&1; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
        FAILURES+=("$test_file")
    fi
    echo ""
done

# 6. Summary
echo "=== Summary ==="
TOTAL=$((PASSED + FAILED))
echo "Passed: $PASSED / $TOTAL"
if [ "$FAILED" -gt 0 ]; then
    echo "Failed: $FAILED"
    for f in "${FAILURES[@]}"; do
        echo "  - $f"
    done
    exit 1
else
    echo "All tests passed."
    exit 0
fi
