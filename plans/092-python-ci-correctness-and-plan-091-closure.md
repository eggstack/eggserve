# Plan 092 — Python CI Correctness and Plan 091 Closure

## Goal

Close the remaining correctness and verification defects left by the implementation of Plan 091 without reintroducing the release-evidence framework that Plan 091 removed.

Plan 091 successfully reduced eggserve's CI and release apparatus, but it is not closed while the entire Python job is marked `continue-on-error` and the repository records known Python test failures as acceptable CI noise. This plan resolves those failures, establishes a reliable installed-wheel test boundary, makes Python packaging and behavior a real blocking check, aligns local and CI verification, removes stale documentation, and produces a defensible closure state.

Completion of this plan means:

- the Rust and Python CI jobs are both blocking;
- wheel build, wheel installation, import validation, CLI smoke, and Python behavioral tests all fail CI normally when broken;
- the known `Method.Get` mismatch, shutdown-deadline hang, corpus-path failure, and request-body capture failure are resolved rather than suppressed;
- Python tests execute against the installed wheel, not the source package by accident;
- test fixtures are resolved from the test checkout through one explicit mechanism rather than package-relative `__file__` assumptions;
- `scripts/verify.sh full` and routine Python CI use the same installed-wheel harness;
- `scripts/verify.sh full` cannot silently skip supported Python verification;
- CI documentation describes the workflow that actually exists;
- Plan 091 is marked closed only after all commands and both blocking jobs pass on the same final commit.

This is a narrow corrective closure pass. It must not rebuild the old gate registry, evidence collector, release workflow, OS matrix, or profile-promotion system.

## Baseline findings

At the start of this plan, the following conditions exist on `main`:

1. `.github/workflows/ci.yml` has two jobs, but the entire `python` job uses `continue-on-error: true`.
2. Because `continue-on-error` is set at job scope, failures in wheel build, wheel installation, imports, CLI smoke, and all Python tests are non-blocking.
3. Commit history explicitly records unresolved failures involving:
   - a stale or incorrect `Method.Get` expectation;
   - `test_shutdown_respects_deadline_with_blocked_handler` hanging on CI;
   - conformance/parity tests locating repository fixtures through package-relative `__file__` paths;
   - a request-body test failing through a missing capture key.
4. The Python job uses `unittest discover` under `crates/eggserve-python/python`, which is also the package source tree. `PYTHONPATH=""` alone does not establish a strong installed-wheel isolation boundary.
5. `scripts/verify.sh full` repeats the wheel build/install/test procedure independently from CI and silently skips Python verification when Python or Maturin is unavailable.
6. `AGENTS.md` and the eggserve development skill describe audit, deny, and package dry-runs as the CI sequence even though routine CI no longer runs them.
7. Documentation still contains references to a Linux/macOS/Windows wheel matrix and profile-promotion terminology that no longer reflects Plan 091 policy.
8. Plan status documentation claims Plan 091 is complete despite the unmet blocking-Python and passing-`full` criteria.

These are closure blockers. They are not grounds to expand CI again.

## Governing invariants

The implementation must preserve these invariants:

1. Routine CI remains one workflow with exactly two Ubuntu jobs: `rust` and `python`.
2. Both jobs are blocking. No job-level or required-step `continue-on-error` is permitted.
3. Python support is a shipped surface, not an informational artifact build.
4. Tests run against the installed wheel while test source and test fixtures remain outside the importable package tree.
5. A failing Python test is fixed, corrected as stale, or deliberately removed with a documented contract-based reason. It is not suppressed.
6. Timing-sensitive tests use deterministic synchronization and bounded waits rather than arbitrary sleeps as their primary oracle.
7. External watchdogs may prevent a hung CI runner, but a watchdog timeout is a test failure, not a success or skip.
8. `verify.sh full` is a pre-release check. Missing required Python tooling is a clear failure, not a silent skip.
9. CI and local full verification share one Python wheel-test implementation.
10. No gate registry, evidence JSON, candidate SHA validation, generated checklist, release workflow, or artifact aggregation is reintroduced.
11. Windows/macOS qualification remains manual and outside routine CI.
12. The project remains scoped to a hardened static-file server and reusable HTTP/filesystem primitives.

## Scope firewall

Do not use this plan to:

- restore `.github/workflows/release.yml`;
- restore scheduled fuzz workflows;
- add an OS matrix to routine CI;
- add per-test evidence files, artifact uploads, or an aggregation job;
- add a replacement for `release/criteria.toml`;
- mark tests skipped, ignored, expected-failure, or non-blocking merely to obtain green CI;
- add broad compatibility aliases without a documented public API reason;
- redesign the Python API beyond the minimum needed to reconcile tests with the documented contract;
- change the supported Python range from CPython 3.14;
- add pytest, tox, nox, Hatch, Poetry, or another test orchestration dependency;
- make `verify.sh` materially larger through duplicated Python packaging logic;
- run deep proxy, fuzz, filesystem-race, benchmark, soak, or Windows qualification suites in routine CI;
- claim Windows hardened support;
- add publication credentials to GitHub Actions;
- publish a release as part of this work.

## Closure blockers to resolve

This plan has six mandatory blocker groups.

### Blocker 1 — Python job is globally non-blocking

Current behavior allows any Python failure to report a successful overall workflow.

Required correction:

- remove job-level `continue-on-error`;
- remove step-level `continue-on-error` from all required Python build, install, smoke, and test steps;
- ensure the Python job returns nonzero on the first failed required phase;
- retain readable direct output without artifact evidence wrappers.

A job that builds and tests a supported distribution cannot be advisory.

### Blocker 2 — Known Python failures are unresolved

The following failures must be reproduced and classified individually:

- method enum/API mismatch;
- shutdown deadline hang;
- conformance/parity fixture path resolution;
- request-body capture/key failure.

Each must be assigned one classification:

- product defect;
- stale test against a superseded contract;
- test harness defect;
- packaging-layout defect;
- nondeterministic synchronization defect.

The implementing agent must not assume the commit-message explanation is correct. Reproduce each failure against the current installed wheel and inspect the implementation and documented API before choosing the correction.

### Blocker 3 — Installed-wheel isolation is weak

The test discovery root currently overlaps the Python package source root.

Required correction:

- move executable Python tests out of `crates/eggserve-python/python/eggserve/` into a non-package test directory, preferably `crates/eggserve-python/tests/`;
- ensure the test directory does not contain an importable top-level `eggserve` package;
- install the wheel into a fresh virtual environment;
- run tests using that virtual environment's interpreter;
- assert that `eggserve.__file__` and `eggserve._native.__file__` resolve under the virtual environment's site-packages, not under the repository's `python/` source tree;
- prevent user-site and ambient `PYTHONPATH` contamination;
- make fixture lookup explicit and independent from installed package paths.

### Blocker 4 — CI and `verify.sh full` duplicate packaging logic

Required correction:

- create one focused helper, preferably `scripts/test-python-wheel.sh`;
- have both routine Python CI and `scripts/verify.sh full` invoke it;
- keep the helper direct, readable, and limited to staging, building, installing, smoke-testing, and running Python tests;
- do not create a generalized task graph or test registry.

### Blocker 5 — Documentation is stale

Required correction:

- distinguish actual routine CI commands from optional local security/package checks;
- remove claims that audit, deny, and package dry-run execute in routine CI unless they are actually restored, which this plan does not recommend;
- distinguish supported wheel platforms from platforms built in routine CI;
- remove active profile-promotion terminology inherited from the deleted framework;
- do not declare Plans 000–092 complete before final verification succeeds.

### Blocker 6 — No same-commit closure proof

Required correction:

- run the required local commands on the final implementation tree;
- obtain a routine GitHub Actions run on the same final commit with both jobs successful;
- do not add committed exact-SHA evidence or regenerate closure reports;
- report the result in the implementation commit/PR description or handoff summary.

The repository must remain free of the evidence churn removed by Plan 091.

## Track A — Reproduce and classify every Python failure

### Objective

Establish the actual failure modes before changing tests or implementation.

### Clean environment

Use a fresh virtual environment outside the package source tree. A representative manual sequence is:

```sh
set -euo pipefail

REPO_ROOT="$(pwd)"
TMP_ROOT="$(mktemp -d)"
python3.14 -m venv "$TMP_ROOT/venv"
PYTHON="$TMP_ROOT/venv/bin/python"

"$PYTHON" -m pip install --disable-pip-version-check maturin==1.14.1

cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve

(
  cd crates/eggserve-python
  "$PYTHON" -m maturin build --release --interpreter "$PYTHON" -o "$TMP_ROOT/dist"
)

"$PYTHON" -m pip install --force-reinstall "$TMP_ROOT"/dist/*.whl
```

Use the Windows-equivalent executable and venv paths when reproducing on Windows. Routine closure is Linux-based; this plan does not require a Windows matrix.

### Import-boundary probe

Before running tests, record and inspect:

```sh
PYTHONNOUSERSITE=1 PYTHONPATH="" "$PYTHON" - <<'PY'
from pathlib import Path
import eggserve
import eggserve._native

print(Path(eggserve.__file__).resolve())
print(Path(eggserve._native.__file__).resolve())
PY
```

Both paths must point into the temporary virtual environment, not the checkout.

### Failure reproduction

Run each affected module or test case independently before running discovery. Use the post-migration test path once Track B is implemented. Until then, invoke the existing module directly against the installed wheel.

Capture only ordinary command output. Do not add evidence JSON.

For each failure, record in working notes:

```text
failure/test
exact command
observed exception or hang
product or harness layer
contract source consulted
chosen correction
regression test
```

This is temporary implementation analysis, not a permanent registry.

### Acceptance criteria

- All four known failure groups are reproducible or proven stale through direct inspection.
- Each failure has a concrete root cause, not only a CI symptom.
- No failure is classified solely from an old commit message.
- No suppression is added during reproduction.
- The implementing agent can state which fixes change product code and which change tests/harnesses.

## Track B — Move Python tests outside the package source tree

### Objective

Create a strong separation between installed package code and repository test code.

### Target layout

Preferred layout:

```text
crates/eggserve-python/
├── pyproject.toml
├── python/
│   └── eggserve/
│       ├── __init__.py
│       ├── __main__.py
│       ├── ... runtime package files ...
│       └── bin/
└── tests/
    ├── __init__.py                 # optional; omit unless required
    ├── _repo.py                    # narrow repository-fixture resolver
    ├── test_primitives.py
    ├── test_api_stability.py
    ├── test_boundary_hardening.py
    ├── test_client_primitives.py
    ├── test_server_integration.py
    ├── test_canonical_conformance.py
    ├── test_api_consumers.py
    ├── test_parity_matrix.py
    ├── test_body_primitives.py
    ├── test_body_wire.py
    ├── test_canonical_request_types.py
    ├── test_body_conformance.py
    ├── test_server_primitives.py
    └── test_server.py
```

Move tests; do not duplicate them and leave two discovery surfaces.

### Packaging behavior

Verify that wheel contents do not include the repository test suite unless intentional package policy explicitly requires it. The preferred result is that `tests/` remains source-distribution/development content and is not installed as `eggserve` runtime code.

If Maturin includes unexpected test files, update package include/exclude settings narrowly. Do not exclude runtime modules.

### Test execution

Run discovery from the external test directory:

```sh
PYTHONNOUSERSITE=1 PYTHONPATH="" \
  "$PYTHON" -m unittest discover \
    -s crates/eggserve-python/tests \
    -p 'test_*.py' \
    -v
```

The helper may `cd` to the repository root first so paths are stable.

### Import assertion

Add a small required test or pre-test smoke check that rejects source-tree imports. It must verify:

- `eggserve.__file__` is outside `crates/eggserve-python/python`;
- `eggserve._native.__file__` is outside the checkout;
- the native extension exists and is importable;
- the installed distribution version is readable where exposed.

Do not rely only on `PYTHONPATH=""`.

### Fixture resolver

Create one narrow test helper for repository fixtures, for example `tests/_repo.py`.

Preferred behavior:

- derive the repository root from the test file's known source layout;
- verify the expected workspace `Cargo.toml` and fixture path exist;
- raise a clear assertion/error when the checkout layout is unavailable;
- accept an explicit `EGGSERVE_REPO_ROOT` override only if useful for external test execution;
- never derive fixture paths from `eggserve.__file__` or any installed package path.

Representative interface:

```python
from pathlib import Path


def repo_root() -> Path:
    root = Path(__file__).resolve().parents[3]
    workspace = root / "Cargo.toml"
    if not workspace.is_file():
        raise RuntimeError(f"eggserve repository root not found: {root}")
    return root


def conformance_corpus() -> Path:
    path = repo_root() / "conformance" / "corpus.json"
    if not path.is_file():
        raise RuntimeError(f"conformance corpus not found: {path}")
    return path
```

Adjust the parent count to the final layout and test it. Centralize this logic; do not repeat parent traversal in multiple tests.

### Acceptance criteria

- No executable tests remain under the importable `python/eggserve/` package tree, except intentional runtime self-tests explicitly justified.
- Test discovery runs from `crates/eggserve-python/tests`.
- Installed package imports resolve to the virtual environment.
- Conformance/parity fixtures resolve from the checkout, not site-packages.
- A missing fixture fails clearly instead of becoming an import error, skip, or non-blocking test.
- Wheel contents contain runtime package files and the packaged CLI, without accidental source test pollution.

## Track C — Correct the method enum/API mismatch

### Objective

Reconcile the Python client test with the actual public API contract without adding an unjustified alias.

### Required investigation

Inspect:

- the PyO3 class names exported from the native module;
- `python/eggserve/__init__.py` exports;
- `docs/python-api.md`;
- API stability tests and consumer fixtures;
- examples using the HTTP client;
- the Rust client primitive type and the canonical HTTP `Method` type.

The current code has distinct concepts:

- canonical `Method` for standard and extension methods;
- client-specific `ClientMethod` exported from `PyMethod`.

Determine whether `test_client_primitives.py` imported the wrong class or whether the documented export is wrong.

### Preferred correction

If the documented and implemented client API is `ClientMethod`, update the client test to:

```python
from eggserve._native import ClientMethod

self.assertIsNotNone(ClientMethod.Get)
self.assertIsNotNone(ClientMethod.Head)
...
```

Do not add `Method.Get` aliases solely to preserve a stale test when canonical `Method` intentionally uses a different Python interface.

If public documentation and existing consumer fixtures instead promise `Method.Get`, correct the binding consistently and add an API compatibility test. The implementing agent must justify this less likely path from repository contracts.

### Required regression coverage

Tests must verify:

- the intended enum/class is exported under the documented name;
- all standard client methods are available;
- `repr`/string behavior matches the documented contract;
- `ClientRequest` accepts the client method type;
- the canonical `Method` and `ClientMethod` remain intentionally distinct if both are public;
- imports from the public `eggserve` package and `_native` layer agree where the package re-exports the type.

### Acceptance criteria

- `test_client_primitives` passes without suppression.
- Documentation, binding class name, package export, and tests agree.
- No compatibility alias is introduced without a documented consumer need.
- The fix does not collapse canonical and client-specific method types accidentally.

## Track D — Fix deterministic shutdown-deadline testing and behavior

### Objective

Ensure a blocked Python handler cannot make the shutdown-deadline test hang indefinitely and that the test verifies the documented runtime contract deterministically.

### Required investigation

Locate `test_shutdown_respects_deadline_with_blocked_handler` and inspect:

- whether it calls graceful `stop()`, `shutdown()`, or `force_shutdown(timeout)`;
- whether the documented API promises bounded return for that method;
- where the GIL is held or released;
- how the Python callback is executed through `spawn_blocking`;
- when the callback semaphore permit is released;
- whether the Rust runtime waits for the blocking task after the force-shutdown deadline;
- whether Python object destruction or test teardown performs an unbounded join;
- whether a client thread remains blocked after the server closes.

The repository documents that a timed-out Python callback may continue in the background and cannot be safely cancelled. The test must respect that contract while still proving that the server's forced-shutdown API returns within its bound.

### Deterministic test design

Replace sleep-first synchronization with events:

1. Handler sets `handler_entered` immediately after starting.
2. Handler waits on `handler_release` with a bounded safety timeout.
3. Test waits for `handler_entered` and fails clearly if the handler never starts.
4. Test invokes the API whose documented contract includes a shutdown deadline.
5. Shutdown invocation runs in the test thread unless doing so prevents an external watchdog.
6. An external watchdog/event bounds the test itself and turns a hang into a failure.
7. Test asserts the shutdown call returns within a tolerant upper bound derived from the configured deadline.
8. Test asserts the listener no longer accepts new connections.
9. Test releases `handler_release` in `finally` so the background callback can terminate.
10. Test joins every helper/client thread and verifies none remain alive.

Representative structure:

```python
handler_entered = threading.Event()
handler_release = threading.Event()
client_done = threading.Event()


def handler(request):
    handler_entered.set()
    handler_release.wait(timeout=TEST_SAFETY_TIMEOUT)
    return Response.text(200, "done")

try:
    # Start request in helper thread.
    self.assertTrue(handler_entered.wait(timeout=START_TIMEOUT))

    started = time.monotonic()
    server.force_shutdown(SHUTDOWN_DEADLINE)
    elapsed = time.monotonic() - started

    self.assertLess(elapsed, SHUTDOWN_DEADLINE + SCHEDULER_TOLERANCE)
finally:
    handler_release.set()
    client_thread.join(timeout=JOIN_TIMEOUT)
    self.assertFalse(client_thread.is_alive())
```

Use actual API names and semantics from the binding.

### Product correction criteria

If the force-shutdown API itself blocks beyond the configured deadline:

- fix the Rust/PyO3 lifecycle path;
- do not hide the issue by increasing CI timeout;
- ensure force shutdown stops accepting connections and returns without waiting indefinitely for uncancellable Python callback completion;
- preserve documented behavior that the callback may continue until it naturally returns;
- ensure callback permits and runtime resources are released when that background callback exits;
- ensure object destruction does not reintroduce an unbounded wait.

If the product behaves correctly and only the test hangs:

- correct the test synchronization and cleanup;
- document why the old test could leave a thread or callback blocked;
- retain regression coverage for the actual deadline contract.

### Required repeated-run verification

Run the targeted test repeatedly, for example:

```sh
for i in $(seq 1 20); do
  PYTHONNOUSERSITE=1 PYTHONPATH="" \
    "$PYTHON" -m unittest \
    tests.test_server_integration.<ClassName>.test_shutdown_respects_deadline_with_blocked_handler \
    -v
done
```

Adjust module/class names to the final test layout.

The repeated run must not hang or intermittently fail on the local Linux environment.

### Acceptance criteria

- The target shutdown test passes as a blocking test.
- The test has bounded waits and unconditional cleanup.
- No helper thread remains alive after the test.
- The shutdown method returns within its documented deadline plus a reasonable scheduling tolerance.
- Increasing the job timeout is not the primary fix.
- The Python job timeout can be reduced from 45 minutes to a proportionate value after the suite is stable.

## Track E — Fix request-body test capture and synchronization

### Objective

Resolve the missing capture key without weakening assertions or replacing failures with dictionary `.get()` defaults.

### Required investigation

Identify the exact failing request-body test and determine why the handler did not populate the expected capture field.

Potential causes to test explicitly:

- the request never reached the handler;
- request body policy rejected the request before handler invocation;
- `req.has_body` or `req.body` did not match the test's assumptions;
- `RequestBody.read()` raised inside the handler;
- response/client completion occurred before the handler updated the shared capture;
- fixed `sleep()` duration was insufficient on CI;
- handler exception was converted to an HTTP 500 and lost from the test thread;
- teardown stopped the server before capture completion.

Do not assume this is only a race until handler exceptions and HTTP response status are inspected.

### Required harness design

For tests that assert handler-captured state, use an explicit result object and event:

```python
handler_done = threading.Event()
captured = {}


def handler(req):
    try:
        captured["declared_length"] = req.body.declared_length
        captured["read_data"] = req.body.read()
        captured["after_read_complete"] = req.body.complete
        return Response.text(200, "ok")
    except BaseException as exc:
        captured["handler_error"] = exc
        raise
    finally:
        handler_done.set()
```

The test must:

- send the request and inspect the response status;
- wait for `handler_done` with a bounded timeout;
- fail with a useful message if the handler did not complete;
- re-raise or fail on `handler_error`;
- assert required capture keys directly only after synchronization;
- avoid arbitrary sleeps as the completion signal.

A helper may centralize this pattern if several body tests need it, but keep the abstraction small and local to the test module.

### Empty-body semantics

Tests involving an empty body must match the actual contract. If `Content-Length: 0` produces `has_body == false` and `body is None`, do not expect the handler to call `body.read()` or populate `read_data` for an empty body.

Choose one contract-based correction:

- update the test to assert `has_body == false` and `body is None`; or
- if the documented contract promises an empty `RequestBody`, correct the implementation.

The repository documentation currently indicates that empty bodies may produce no body object. Verify the definitive contract before changing behavior.

### Required coverage

Retain or add tests for:

- non-empty buffered body read;
- bytes received and completion state after read;
- one-shot second-read rejection;
- read-then-iterate rejection;
- iterate-then-read rejection;
- empty body semantics;
- handler exception visibility;
- request body limit rejection;
- body timeout behavior where already covered.

### Acceptance criteria

- No request-body test fails through a missing capture key.
- No required assertion is weakened to `.get(..., default)` merely to avoid `KeyError`.
- Handler completion is event-driven.
- Handler exceptions are visible in test failure output.
- Empty-body expectations agree with the documented API.
- The request-body module passes repeatedly against the installed wheel.

## Track F — Create one installed-wheel verification harness

### Objective

Use one direct helper for both routine Python CI and local `verify.sh full`.

### New helper

Create `scripts/test-python-wheel.sh` or an equivalently named focused script.

Required responsibilities:

1. locate the repository root;
2. require the supported Python interpreter;
3. require Maturin or install it only when explicitly requested by CI setup;
4. create a temporary virtual environment;
5. clean the temporary environment on every exit;
6. build the release CLI binary once;
7. stage the platform-appropriate CLI into the package once;
8. build one wheel once;
9. install that wheel into the temporary virtual environment;
10. assert imports come from the installed wheel;
11. run installed CLI/package smoke checks;
12. run Python test discovery once from the external test directory;
13. propagate the first nonzero exit status;
14. stream output directly.

The helper must not:

- produce JSON evidence;
- upload artifacts;
- parse TOML gate definitions;
- create manifests;
- modify package versions;
- publish anything;
- silently skip tests;
- install arbitrary system packages;
- retain virtual environments or staged generated files after failure unless explicitly useful and documented.

### Tool ownership

CI should install the pinned Maturin version before invoking the helper. Locally, the helper may require `maturin` and print a direct installation instruction when absent.

Use `python -m pip` rather than ambient `pip` after the temporary environment is created.

### Platform-aware binary staging

The helper should select:

- `target/release/eggserve` and package destination `eggserve/bin/eggserve` on Unix;
- `target/release/eggserve.exe` and package destination `eggserve/bin/eggserve.exe` on Windows if the helper is later used manually there.

Routine CI remains Linux-only.

### Smoke checks

At minimum, run against the installed package:

```sh
"$PYTHON" -c 'import eggserve, eggserve._native'
"$PYTHON" -m eggserve --help
```

Also verify the bundled CLI is present and executable through the package's supported public access path.

Preferred functional smoke:

- create a temporary directory with one file;
- start the installed CLI on loopback with an ephemeral or safely selected port;
- fetch the file using Python stdlib;
- assert exact body bytes;
- terminate and reap the process reliably.

Reuse `tests/installed-binary-qual.sh` only if it can be invoked cleanly without old gate terminology and without duplicating server smoke logic. Otherwise keep the Python smoke small and update the installed-binary script comments to describe it as a manual deep check, not a deleted release gate.

### Cleanup

Use a cleanup function defined before installing the trap. Guard process termination and temporary directory removal so cleanup does not change a successful exit into failure.

Remove staged package binaries after the run if they are generated files rather than tracked fixtures. Ensure `git status --short` remains clean after local `full` verification.

### Acceptance criteria

- CI and `verify.sh full` call the same helper.
- The helper creates a fresh isolated environment per run.
- Wheel build/install occurs once per invocation.
- Import paths prove installed-wheel use.
- CLI/package smoke is blocking.
- Python discovery is blocking and executes once.
- Cleanup leaves no untracked wheel, virtual environment, staged binary, `__pycache__`, or `.pyc` files in the checkout.
- A deliberately failing Python test makes the helper exit nonzero.

## Track G — Make routine Python CI a true blocking job

### Objective

Restore Python package correctness as a required routine CI result without expanding the workflow.

### Target workflow shape

Keep `.github/workflows/ci.yml` to two jobs:

- `rust`;
- `python`.

Remove `needs: [rust]` unless Python truly consumes Rust job output. The jobs should normally run concurrently to reduce wall-clock time.

The Python job should be shaped approximately as follows:

```yaml
python:
  name: python
  runs-on: ubuntu-latest
  timeout-minutes: 20
  env:
    PYO3_USE_ABI3_FORWARD_COMPATIBILITY: "1"
    PYTHONNOUSERSITE: "1"
  steps:
    - uses: actions/checkout@<pinned-sha>
    - uses: dtolnay/rust-toolchain@<pinned-sha>
      with:
        toolchain: stable
    - uses: actions/setup-python@<pinned-sha>
      with:
        python-version: "3.14"
    - uses: Swatinem/rust-cache@<pinned-sha>
    - name: Install Maturin
      run: python -m pip install maturin==1.14.1
    - name: Build, install, smoke, and test wheel
      run: bash scripts/test-python-wheel.sh
```

Exact action SHAs may remain at their current pinned values.

### Required failure behavior

The following must fail the Python job:

- Rust extension compile error;
- CLI release build error;
- missing staged binary;
- wheel build error;
- wheel install error;
- source-tree import leakage;
- native module import error;
- CLI smoke failure;
- any Python test failure;
- test hang reaching the job timeout.

No required step may use `continue-on-error`.

### Timeout

After fixing the shutdown test, reduce the job timeout from 45 minutes. A target of 15–20 minutes is appropriate unless measured clean runs demonstrate a justified higher bound.

Do not set a short timeout that creates routine false failures. The objective is proportionality, not an arbitrary number.

### Job dependency

The Python job builds Rust components itself and currently does not consume an artifact from `rust`. Therefore, remove `needs: [rust]` so both jobs begin together.

Do not add artifact sharing merely to avoid this build. The added orchestration cost is not justified for two simple jobs.

### Controlled failure validation

During implementation, verify failure propagation by temporarily introducing one harmless failing assertion or invoking the helper with a test-only failure hook. Confirm:

- Python job is red;
- overall workflow is red;
- failure output is visible directly;
- no artifact download is required.

Revert the controlled failure before the final commit.

### Acceptance criteria

- `python` has no job-level `continue-on-error`.
- No required Python step has `continue-on-error`.
- `rust` and `python` are the only routine jobs.
- Both jobs are blocking.
- Jobs run concurrently unless a concrete dependency is documented.
- Python job completes within a proportionate timeout.
- A Python test failure makes the overall workflow fail.
- No evidence or release machinery returns.

## Track H — Align `verify.sh full` with release policy

### Objective

Make local full verification a reliable pre-release command rather than a best-effort check.

### Required changes

Replace the duplicated Python block in `scripts/verify.sh` with:

```sh
run bash "$SCRIPT_DIR/test-python-wheel.sh"
```

`full` must require Python wheel verification because eggserve still ships a Python package.

Remove behavior equivalent to:

```text
Python/maturin not available — skipping wheel tests
```

Instead, fail with a concise prerequisite message, for example:

```text
Python 3.14 and maturin 1.14.1 are required for `verify.sh full`.
Use `verify.sh fast` for Rust-only development checks.
```

### Mode semantics

- `fast`: Rust-only routine edit loop; may run without Python/Maturin.
- `full`: all routine Rust checks, feature tests, installed Python wheel verification, and package dry-runs; missing prerequisites fail.
- `deep`: inherits `full`, then runs applicable expensive suites.

Optional external proxy tools in `deep` may remain conditional, with explicit not-run messages. Required Python checks in `full` may not.

### Working-tree cleanliness

Running `full` must not leave:

- staged package binaries;
- wheel files;
- virtual environments;
- `__pycache__` directories;
- `.pyc` files;
- modified lockfiles;
- generated manifests.

The helper should use temporary output directories where possible.

### Acceptance criteria

- There is one Python packaging/test implementation shared by CI and `full`.
- `full` fails when supported Python verification cannot run.
- `fast` remains quick and Rust-only.
- `deep` retains manual expensive suites.
- `git status --short` is unchanged after successful `full`.
- A Python test failure causes `full` to exit nonzero.

## Track I — Reconcile active documentation

### Objective

Make current instructions describe current CI and verification behavior precisely.

### `AGENTS.md`

Separate commands into:

1. **Routine CI commands**
   - Rust workflow commands;
   - `scripts/test-python-wheel.sh` through the Python job.

2. **Local verification**
   - `verify.sh fast`;
   - `verify.sh full`;
   - `verify.sh deep`.

3. **Optional manual security/package commands**
   - `cargo audit`;
   - `cargo deny check`;
   - package verification helper where not already in `full`.

Do not say CI runs audit/deny if it does not.

### Development skill

Apply the same distinction in `.opencode/skills/eggserve-dev/SKILL.md`.

Do not state that Plan 092 is complete until closure verification passes.

### Wheel platform language

Replace statements such as "on the Linux, macOS, and Windows wheel matrix" with accurate language:

- CPython 3.14 is the supported Python version;
- routine CI builds/tests the Linux wheel;
- macOS and Windows wheels are built and checked manually when preparing those distributions;
- no routine platform matrix exists.

Do not remove platform classifiers merely because routine CI is Linux-only. Classifiers describe intended package support, not CI shape; verify they remain truthful.

### Profile terminology

Remove active references to machine "profile promotion" decisions. Replace with direct human-readable status, such as:

- Windows is functional and has handle-relative confinement;
- adversarial qualification and independent review remain incomplete;
- do not use Windows for untrusted public content until that work is completed.

Do not restore support-profile TOML or evidence state.

### Installed binary script

Update comments in `tests/installed-binary-qual.sh` so they no longer claim to satisfy a deleted `artifact.installed-binaries` gate.

Describe it as a manual installed-artifact smoke/qualification helper retained after Plan 091.

### Plan status

Before final closure, active docs should say:

```text
Plan 091 simplification is implemented; Plan 092 closes the remaining Python CI correctness gaps.
```

Only after all Definition of Done items pass may the plan status be changed to Plans 000–092 implementation-complete.

### Acceptance criteria

- Active documentation no longer claims audit/deny/package checks run in routine CI unless they do.
- Active documentation no longer claims a routine three-platform wheel matrix.
- Active documentation no longer uses deleted gate/profile-promotion mechanisms as current policy.
- Installed-binary helper comments contain no deleted gate ID.
- Plan status is truthful throughout implementation.

## Track J — Final closure verification

### Objective

Demonstrate that Plan 091 and Plan 092 criteria are satisfied on one final implementation commit without creating permanent evidence machinery.

### Required local commands

From a clean checkout on Linux with CPython 3.14 and Maturin 1.14.1:

```sh
git status --short
./scripts/verify.sh fast
./scripts/verify.sh full
git status --short
```

Both `git status` outputs must be empty.

Run targeted repeated tests:

```sh
# Exact module paths depend on final test layout.
for i in $(seq 1 20); do
  bash scripts/test-python-wheel.sh --test \
    tests.test_server_integration.<Class>.test_shutdown_respects_deadline_with_blocked_handler
done

for i in $(seq 1 10); do
  bash scripts/test-python-wheel.sh --test tests.test_body_primitives
done
```

A `--test` selector is optional. If adding it materially complicates the helper, create the venv once in working verification and invoke the interpreter directly. Do not inflate the permanent script merely for repetition convenience.

Run repository searches:

```sh
rg -n "continue-on-error" .github/workflows/ci.yml
rg -n "Method\.Get|Method\.Head|Method\.Post|Method\.Put|Method\.Delete|Method\.Patch" crates/eggserve-python/tests crates/eggserve-python/python
rg -n "release/criteria\.toml|ci-gate-evidence|release_criteria|evidence-aggregate" .github scripts AGENTS.md .opencode docs README.md
rg -n "artifact\.installed-binaries|profile promotion|wheel matrix" AGENTS.md .opencode docs README.md tests
```

Interpretation:

- `continue-on-error` search must return no result in routine CI;
- method enum references must match the intended public type;
- deleted release machinery may appear in historical plans or historical notes, but not active workflow/instruction paths;
- installed-binary gate and wheel-matrix wording must be removed from active docs.

### Required CI result

Push the final implementation commit and verify:

- workflow parses and starts;
- `rust` succeeds;
- `python` succeeds;
- neither job is neutral/allowed-failure;
- no third routine job appears;
- no evidence artifact is uploaded;
- no release workflow runs;
- direct logs are sufficient to diagnose any failure.

### Same-commit requirement

If any code, test, workflow, or documentation change is made after the successful local/CI run, rerun the affected closure commands. Do not claim closure using a run from an earlier commit.

This does not require committing the final SHA into the repository.

### Acceptance criteria

- `verify.sh fast` passes.
- `verify.sh full` passes.
- The targeted shutdown test passes 20 consecutive runs.
- The request-body module passes 10 consecutive runs.
- Working tree remains clean after full verification.
- Both routine CI jobs pass on the final commit.
- Python is not allowed to fail.
- No stale active references remain.

## Ordered implementation sequence

Implement this plan in small commits that preserve bisectability.

### Commit 1 — Externalize tests and add installed-wheel harness

- move tests from `python/eggserve` to `crates/eggserve-python/tests`;
- add the centralized repository fixture resolver;
- add `scripts/test-python-wheel.sh`;
- add import-boundary and CLI smoke checks;
- update `verify.sh full` to call the helper;
- do not remove Python `continue-on-error` yet if the known tests still fail on this intermediate commit.

Acceptance:

- the helper builds, installs, and runs the suite against site-packages;
- fixture path failures are clear;
- no source import leakage occurs.

### Commit 2 — Correct method and fixture contract defects

- resolve `ClientMethod` versus canonical `Method` according to documented API;
- fix conformance/parity fixture resolution through the test helper;
- add regression coverage for imports and fixture paths.

Acceptance:

- client primitive, canonical conformance, and parity tests pass without suppression.

### Commit 3 — Correct shutdown and request-body failures

- reproduce the shutdown hang and body capture failure;
- fix product behavior where required;
- replace sleep-based completion assumptions with event-driven synchronization;
- add bounded cleanup and repeated-run coverage.

Acceptance:

- targeted tests pass repeatedly and cannot hang indefinitely.

### Commit 4 — Make Python CI blocking

- remove job-level and step-level `continue-on-error`;
- remove unnecessary `needs: [rust]`;
- call the shared wheel helper;
- reduce timeout to a measured proportionate value;
- validate controlled failure propagation and revert the temporary failure.

Acceptance:

- a Python failure makes the workflow fail;
- both jobs remain the only routine jobs.

### Commit 5 — Documentation reconciliation

- update `AGENTS.md`;
- update the development skill;
- correct wheel platform/matrix language;
- remove active profile-promotion language;
- update installed-binary helper comments;
- state Plan 092 is pending final verification.

Acceptance:

- active instructions match actual CI and local verification.

### Commit 6 — Final closure

- run all Track J commands;
- obtain successful blocking CI on the final commit;
- update plan status to implementation-complete only after success;
- include a concise result summary in the commit or PR description.

Acceptance:

- every Definition of Done item below is satisfied.

## Explicit rejection criteria

Reject an implementation that:

- leaves job-level `continue-on-error` on Python;
- makes only wheel build blocking while behavioral tests remain advisory;
- marks failing tests skipped, ignored, expected-failure, or allowed-failure;
- increases the Python timeout without fixing the hang;
- leaves tests inside the importable package and claims `PYTHONPATH=""` is sufficient isolation;
- runs conformance tests against source code while other tests run against the installed wheel without an explicit contract reason;
- changes missing-key assertions to `.get()` defaults without fixing synchronization;
- uses arbitrary longer sleeps as the main race fix;
- adds `Method.Get` aliases only to satisfy a stale test;
- duplicates wheel build/install logic in CI and `verify.sh`;
- allows `verify.sh full` to skip Python verification;
- leaves generated binaries, wheels, venvs, or bytecode in the working tree;
- adds a third routine CI job;
- restores an OS matrix;
- restores evidence artifacts or release gates;
- restores automated publication;
- marks Plan 092 complete before both blocking jobs pass.

## Definition of Done

Plan 092 is complete only when every item below is true.

### Python correctness

- [ ] The method enum/API mismatch has a documented root cause and passing regression test.
- [ ] `test_client_primitives` passes against the installed wheel.
- [ ] Canonical conformance tests pass against the installed wheel with checkout fixtures.
- [ ] Parity tests pass against the installed wheel with checkout fixtures.
- [ ] The shutdown-deadline test passes without hanging.
- [ ] The shutdown-deadline test passes 20 consecutive local runs.
- [ ] Request-body tests use deterministic handler-completion synchronization.
- [ ] Request-body tests expose handler exceptions directly.
- [ ] The request-body module passes 10 consecutive local runs.
- [ ] Empty-body semantics agree across code, tests, and documentation.
- [ ] No known Python failure is suppressed.

### Installed-wheel boundary

- [ ] Executable tests live outside `python/eggserve`.
- [ ] A fresh virtual environment is created for wheel verification.
- [ ] Wheel build and install occur once per helper invocation.
- [ ] `eggserve.__file__` resolves under the virtual environment.
- [ ] `eggserve._native.__file__` resolves under the virtual environment.
- [ ] Neither import resolves under the checkout package source.
- [ ] Test fixtures resolve from the checkout through one explicit helper.
- [ ] Missing fixtures fail clearly.
- [ ] Installed CLI/package smoke checks are blocking.

### CI

- [ ] `.github/workflows/ci.yml` still has exactly two jobs.
- [ ] Both jobs run on Ubuntu.
- [ ] `rust` is blocking.
- [ ] `python` is blocking.
- [ ] No required step uses `continue-on-error`.
- [ ] Rust and Python run concurrently unless a documented dependency exists.
- [ ] Python job calls the shared installed-wheel helper.
- [ ] Python job timeout is proportionate after the hang fix.
- [ ] A controlled Python failure was verified to fail the overall workflow and then reverted.
- [ ] Both jobs pass on the final implementation commit.
- [ ] No evidence upload, aggregation, release, matrix, benchmark, soak, proxy, or publication work returns to routine CI.

### Local verification

- [ ] `scripts/test-python-wheel.sh` is the single wheel-test implementation.
- [ ] `scripts/verify.sh full` invokes that helper.
- [ ] `verify.sh full` fails clearly when required Python/Maturin tooling is missing.
- [ ] `verify.sh fast` remains Rust-only.
- [ ] `verify.sh deep` retains manual expensive suites.
- [ ] `verify.sh fast` passes on the final commit.
- [ ] `verify.sh full` passes on the final commit.
- [ ] A Python test failure makes `verify.sh full` fail.
- [ ] `git status --short` is clean before and after `verify.sh full`.

### Documentation

- [ ] `AGENTS.md` accurately separates routine CI, local verification, and optional manual checks.
- [ ] The development skill accurately separates routine CI, local verification, and optional manual checks.
- [ ] No active documentation claims audit or deny runs in routine CI unless restored by a separate decision.
- [ ] No active documentation claims a routine Linux/macOS/Windows wheel matrix.
- [ ] Supported wheel platforms are described independently from routine CI platforms.
- [ ] Active documentation contains no machine profile-promotion requirement.
- [ ] `tests/installed-binary-qual.sh` no longer claims to satisfy a deleted gate.
- [ ] Plan status is updated to Plans 000–092 implementation-complete only after verification succeeds.

### Plan 091 closure

- [ ] Plan 091's two-blocking-job requirement is satisfied.
- [ ] Plan 091's installed Python package test requirement is satisfied.
- [ ] Plan 091's `verify.sh full` passing requirement is satisfied.
- [ ] Plan 091's direct failure-diagnosis requirement is satisfied.
- [ ] Plan 091's stale-documentation cleanup requirement is satisfied.
- [ ] Manual release policy remains unchanged.
- [ ] Automated crates.io, PyPI, and GitHub Release publication remains absent.
- [ ] The release-evidence subsystem remains deleted.

## Handoff note

The purpose of this pass is not to make CI elaborate again. The correct result remains an 80–120 line workflow with two direct jobs. The work belongs in product tests, test layout, one small wheel helper, and precise documentation.

The main implementation error to avoid is treating Python as optional because the Rust core is healthy. Eggserve ships a Python package and bundled CLI. Its wheel must build, install, import, execute, and pass its supported tests as a blocking condition.

When deciding between alternatives, prefer the outcome that:

1. fixes the root cause;
2. keeps Python blocking;
3. tests the installed artifact;
4. uses deterministic synchronization;
5. shares one helper between CI and local full verification;
6. keeps the workflow small;
7. leaves the working tree clean;
8. does not restore release ceremony.
