#!/usr/bin/env bash
set -euo pipefail

# ci-gate-evidence.sh — Run a gate command and emit structured JSON evidence.
#
# Usage:
#   ci-gate-evidence.sh <gate-id> <result-class> <command...>
#
# <result-class> is the evidence class (e.g. "ci-log", "test-output").
# The command is executed and the exit code is captured.
# Evidence is written to target/release-evidence/ci/<gate-id>.json.
#
# Environment variables:
#   GITHUB_SHA         — commit SHA (set by GitHub Actions)
#   GITHUB_RUN_ID      — workflow run ID
#   GITHUB_JOB         — job name
#   GITHUB_WORKFLOW    — workflow name
#   GITHUB_REF         — ref that triggered the workflow
#   GITHUB_EVENT_NAME  — event type (push, pull_request, etc.)
#   GITHUB_SERVER_URL  — GitHub server URL
#   RUNNER_OS          — runner OS (Linux, macOS, Windows)
#   RUNNER_ARCH        — runner architecture

SKIP_MODE=""
SKIP_REASON=""
if [ "${1:-}" = "--skip" ]; then
  if [ $# -lt 3 ]; then
    echo "Usage: $0 --skip <reason> <gate-id> <result-class>" >&2
    exit 1
  fi
  SKIP_MODE="true"
  SKIP_REASON="$2"
  GATE_ID="$3"
  RESULT_CLASS="${4:-ci-log}"
  shift 4
else
  if [ $# -lt 3 ]; then
    echo "Usage: $0 <gate-id> <result-class> <command...>" >&2
    echo "       $0 --skip <reason> <gate-id> <result-class>" >&2
    exit 1
  fi
  GATE_ID="$1"
  RESULT_CLASS="$2"
  shift 2
fi

# Collect metadata
COMMIT_SHA="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo 'unknown')}"
RUN_ID="${GITHUB_RUN_ID:-local}"
JOB_NAME="${GITHUB_JOB:-local}"
WORKFLOW_NAME="${GITHUB_WORKFLOW:-local}"
EVENT_NAME="${GITHUB_EVENT_NAME:-local}"
SERVER_URL="${GITHUB_SERVER_URL:-https://github.com}"
RUNNER_OS="${RUNNER_OS:-$(uname -s)}"
RUNNER_ARCH="${RUNNER_ARCH:-$(uname -m)}"
TARGET_TRIPLE="$(rustc -vV 2>/dev/null | grep '^host:' | sed 's/^host: //' || echo 'unknown')"
RUST_VERSION="$(rustc --version 2>/dev/null || echo 'unknown')"
PYTHON_VERSION="$(python3 --version 2>/dev/null || echo 'unknown')"

EVIDENCE_DIR="target/release-evidence/ci"
mkdir -p "$EVIDENCE_DIR"

# Handle skip mode: emit a not-applicable record without executing any command
if [ -n "$SKIP_MODE" ]; then
  START_EPOCH="$(date +%s)"
  START_ISO="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  END_EPOCH="$START_EPOCH"
  END_ISO="$START_ISO"
  DURATION=0
  EXIT_CODE=0
  RESULT="not-applicable"

  WORKFLOW_URL=""
  if [ "$RUN_ID" != "local" ]; then
    WORKFLOW_URL="${SERVER_URL}/${GITHUB_REPOSITORY:-unknown}/actions/runs/${RUN_ID}"
  fi

  cat > "${EVIDENCE_DIR}/${GATE_ID}.json" <<EOFEVIDENCE
{
  "schema_version": "1.0.0",
  "gate_id": "${GATE_ID}",
  "result": "${RESULT}",
  "evidence_class": "GITHUB",
  "command": "",
  "exit_code": 0,
  "start_time": "${START_ISO}",
  "end_time": "${END_ISO}",
  "duration_secs": 0,
  "commit_sha": "${COMMIT_SHA}",
  "dirty_tree": false,
  "os": "${RUNNER_OS}",
  "arch": "${RUNNER_ARCH}",
  "tool_versions": {},
  "features": [],
  "target_triple": "${TARGET_TRIPLE}",
  "log_path": null,
  "skip_reason": "${SKIP_REASON}",
  "invalidation_info": null,
  "workflow_run_url": "${WORKFLOW_URL}",
  "job_id": "${JOB_NAME}",
  "runner_os": "${RUNNER_OS}",
  "artifact_ids": []
}
EOFEVIDENCE
  echo "Evidence written (not-applicable): ${EVIDENCE_DIR}/${GATE_ID}.json"
  exit 0
fi

START_EPOCH="$(date +%s)"
START_ISO="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

EXIT_CODE=0
RESULT="passed"
"$@" || EXIT_CODE=$?

END_EPOCH="$(date +%s)"
DURATION=$((END_EPOCH - START_EPOCH))
END_ISO="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

if [ "$EXIT_CODE" -ne 0 ]; then
  RESULT="failed"
fi

WORKFLOW_URL=""
if [ "$RUN_ID" != "local" ]; then
  WORKFLOW_URL="${SERVER_URL}/${GITHUB_REPOSITORY:-unknown}/actions/runs/${RUN_ID}"
fi

# Write evidence JSON
cat > "${EVIDENCE_DIR}/${GATE_ID}.json" <<EOFEVIDENCE
{
  "schema_version": "1.0.0",
  "gate_id": "${GATE_ID}",
  "result": "${RESULT}",
  "evidence_class": "GITHUB",
  "command": $(printf '%s' "$*" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))' 2>/dev/null || echo '"$*"'),
  "exit_code": ${EXIT_CODE},
  "start_time": "${START_ISO}",
  "end_time": "${END_ISO}",
  "duration_secs": ${DURATION},
  "commit_sha": "${COMMIT_SHA}",
  "dirty_tree": false,
  "os": "${RUNNER_OS}",
  "arch": "${RUNNER_ARCH}",
  "tool_versions": {
    "rustc": "${RUST_VERSION}",
    "python3": "${PYTHON_VERSION}"
  },
  "features": [],
  "target_triple": "${TARGET_TRIPLE}",
  "log_path": null,
  "skip_reason": null,
  "invalidation_info": null,
  "workflow_run_url": "${WORKFLOW_URL}",
  "job_id": "${JOB_NAME}",
  "runner_os": "${RUNNER_OS}",
  "artifact_ids": []
}
EOFEVIDENCE

echo "Evidence written: ${EVIDENCE_DIR}/${GATE_ID}.json"
exit "$EXIT_CODE"
