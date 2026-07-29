# Plan 093 — Final Python Test and Verification Closure

## Goal

Close the remaining defects left after Plan 092 without expanding routine CI, restoring the deleted evidence framework, or weakening test coverage through `unittest.skip`.

Plan 092 substantially corrected the Python verification boundary:

- routine CI now has two direct blocking Ubuntu jobs;
- Python tests run against an installed wheel in a fresh virtual environment;
- CI and `scripts/verify.sh full` share `scripts/test-python-wheel.sh`;
- the client-method mismatch, fixture path assumptions, shutdown test, and request-body synchronization were materially corrected;
- Python failures now propagate normally.

The line of work is not closed because the implementation then converted additional exposed failures into skipped tests, retained active profile-promotion terminology, and marked Plans 000–092 complete without independently establishing all same-commit closure results required by Plan 092.

This plan is the final corrective pass. Completion means:

- no supported Python behavior is hidden behind an unconditional skip;
- every currently skipped Python test has an explicit, reviewable disposition;
- observer delivery is either fixed and tested or removed from the supported API/documentation through a deliberate contract decision;
- file-stream concurrency coverage exists at a deterministic layer rather than being disabled as a class;
- active documentation uses direct platform/security statements, not the deleted profile-promotion process;
- the Python wheel helper enforces a clean import environment consistently in CI and locally;
- required repeated tests and `verify.sh` modes pass on the final tree;
- both blocking routine CI jobs pass on the final commit;
- Plans 091–093 may then be considered closed.

This plan must remain a narrow closure pass. It does not authorize a new CI architecture, release process, qualification registry, or feature track.

## Current baseline

At the start of Plan 093, `main` has the following relevant state.

### Correctly landed foundations

1. `.github/workflows/ci.yml` contains exactly two jobs: `rust` and `python`.
2. Both jobs run on Ubuntu and are blocking.
3. The Python job has no `needs: [rust]` dependency and can run concurrently.
4. The Python job calls `scripts/test-python-wheel.sh`.
5. `scripts/test-python-wheel.sh`:
   - builds the release CLI;
   - stages the CLI into the Python package;
   - builds one wheel;
   - creates one temporary virtual environment;
   - installs the wheel;
   - checks package and native-extension import paths;
   - runs CLI smoke checks;
   - runs the Python suite from `crates/eggserve-python/tests`.
6. `scripts/verify.sh full` calls the same helper.
7. Python tests no longer live under the importable `python/eggserve` package tree.
8. Repository conformance fixtures resolve through `crates/eggserve-python/tests/_repo.py`.
9. Client tests use `ClientMethod`, not the unrelated canonical `Method` API.
10. Request-body tests now use completion events and surface callback exceptions.
11. The shutdown-deadline test uses bounded event synchronization and `force_shutdown`.

These foundations must be preserved.

### Remaining closure defects

1. `crates/eggserve-python/tests/test_server_integration.py` marks the entire `TestFileStreamSemaphore` class skipped.
2. `crates/eggserve-python/tests/test_server_primitives.py` skips observer-delivery tests because observer events are not delivered by the current runtime.
3. The implementation summary reports 18 skipped tests rather than resolving or deliberately replacing the coverage.
4. Plan 092 explicitly rejected obtaining closure through skipped/ignored/expected-failure tests.
5. Active documentation still uses phrases such as “profile promotion decision awaited.”
6. Active plan status text declares Plans 000–092 implementation-complete before the outstanding skip dispositions and final verification are complete.
7. `scripts/test-python-wheel.sh` relies partly on the caller for `PYTHONNOUSERSITE` and does not explicitly neutralize ambient `PYTHONPATH` for every local invocation.
8. `scripts/verify.sh full` checks for `python3` even though the shared helper defaults to `python3.14`, creating a needless alias mismatch.
9. The required 20 consecutive shutdown-test runs and 10 consecutive request-body module runs are not documented as completed on the final implementation tree.
10. Successful `rust` and `python` Actions jobs on the final implementation commit have not been established in the repository handoff record available for review.

## Governing invariants

The implementation must preserve all of the following.

1. Routine CI remains one workflow with exactly two direct Ubuntu jobs.
2. Both routine jobs remain blocking.
3. No required Python build, install, smoke, or test step uses `continue-on-error`.
4. No failing supported test is converted to skip, expected failure, retry-until-green, or advisory status.
5. Environment-dependent qualification may be moved out of routine CI only when deterministic routine coverage replaces the substantive contract.
6. Product behavior that is documented and publicly exposed must be fixed or deliberately removed; it must not remain advertised while its tests are skipped.
7. Test replacements must preserve the behavioral assertion, not merely test that a constructor accepts an option.
8. Timing and concurrency tests must use explicit synchronization or a controllable test double as their primary oracle.
9. Routine CI must not gain an OS matrix, proxy installation, fuzz campaign, benchmark, soak, SBOM, publication, or evidence aggregation.
10. No release workflow or registry credential returns to GitHub Actions.
11. `scripts/test-python-wheel.sh` remains a focused helper, not a generalized test framework.
12. `verify.sh fast` remains Rust-only and suitable for the edit loop.
13. `verify.sh full` remains the complete local pre-release path and must fail when required Python verification cannot run.
14. Manual release policy remains unchanged.
15. Windows remains described conservatively and is not promoted to hardened/public-untrusted support by this work.
16. Historical plans may retain historical terminology, but active instructions and support claims must not depend on deleted profile/evidence machinery.

## Scope firewall

Do not use Plan 093 to:

- restore `.github/workflows/release.yml`;
- restore scheduled fuzz workflows;
- add another routine workflow or third routine job;
- add a Linux/macOS/Windows routine matrix;
- restore `release/criteria.toml`, evidence JSON, candidate-SHA validation, or generated checklists;
- add pytest, tox, nox, Hatch, Poetry, or another orchestration dependency;
- add flaky-test retry actions;
- mark additional tests skipped;
- replace `unittest.skip` with environment-variable skips that are always active in CI;
- hide failures behind broad exception handling;
- weaken assertions to “did not crash” when the contract requires event delivery or concurrency limiting;
- expand observer functionality beyond the currently documented callback contract;
- redesign the server runtime generally;
- redesign file streaming generally unless a demonstrated product defect requires a minimal correction;
- add permanent debug logging or test-only public APIs;
- publish a release;
- change crate or package versions;
- claim cross-platform closure not actually run.

## Required file inventory

The implementing agent must inspect at least these files before changing code:

- `.github/workflows/ci.yml`
- `scripts/test-python-wheel.sh`
- `scripts/verify.sh`
- `crates/eggserve-python/tests/test_server_integration.py`
- `crates/eggserve-python/tests/test_server_primitives.py`
- `crates/eggserve-python/tests/test_body_primitives.py`
- `crates/eggserve-python/tests/test_canonical_conformance.py`
- `crates/eggserve-python/tests/test_parity_matrix.py`
- `crates/eggserve-python/src/server.rs`
- `crates/eggserve-core/src/server/` and the concrete runtime modules used for file-body streaming and shutdown
- `crates/eggserve-core/src/ops/` or the actual operational event implementation
- `AGENTS.md`
- `.opencode/skills/eggserve-dev/SKILL.md`
- `README.md`
- `docs/deployment.md`
- `docs/python-api.md`
- `docs/python-packaging.md`
- `docs/release-contract.md`
- `tests/installed-binary-qual.sh`

Search for all skip and legacy terminology before implementation:

```sh
rg -n "unittest\.skip|skipUnless|skipIf|expectedFailure" crates/eggserve-python/tests
rg -n "profile promotion|profile decision|candidate profile|support-profiles" \
  AGENTS.md .opencode README.md docs tests scripts .github
rg -n "Plans 000.*092|implementation-complete|Plan 092" \
  AGENTS.md .opencode README.md docs
rg -n "continue-on-error" .github/workflows
```

`skipUnless(NATIVE_AVAILABLE, ...)` must be evaluated separately from unconditional product skips. The installed-wheel harness requires the native module, so a missing native extension should normally fail before tests execute. Do not count a condition that can never be false in the supported harness as meaningful coverage.

## Track A — Build an explicit skip disposition table

### Objective

Ensure no skipped test disappears through an ad hoc edit and every test has a justified final location.

### Temporary working table

Create a temporary working note during implementation with these columns:

```text
file
test class or method
current skip reason
behavioral contract
current implementation path
root cause
final disposition
replacement test path
routine/manual classification
```

This table is an implementation aid only. Do not create a new permanent gate registry.

### Allowed final dispositions

Every unconditional skipped test must end in exactly one of these states:

1. **Product fixed; test enabled**
   - use when the documented behavior is supported and the implementation is defective.

2. **Invalid test replaced at a deterministic layer**
   - use when the asserted contract is real but the existing external mechanism cannot observe it reliably;
   - preserve the behavioral assertion in a lower-level Rust or Python test with explicit synchronization.

3. **Stale contract test deleted with documentation correction**
   - use only when the behavior is intentionally unsupported or no longer part of the public/experimental contract;
   - remove or correct every active documentation/API statement that claimed the behavior;
   - explain the deletion in the implementation commit.

4. **Environment-specific manual qualification extracted**
   - use only for a behavior that genuinely requires host/kernel/platform conditions unavailable in routine CI;
   - add deterministic routine coverage for the core invariant first;
   - retain a focused manual script or clearly documented direct command for the environment-specific observation.

### Forbidden dispositions

Do not:

- retain the unconditional skip;
- move the same skip to another file;
- change it to `skipIf(os.environ...)` with CI always setting the skip condition;
- catch assertion failures and print warnings;
- remove the test without identifying its contract;
- reduce a concurrency test to constructor validation;
- claim a product defect is merely flaky without reproduction.

### Acceptance criteria

- Every unconditional Python skip is listed and classified.
- Every skipped test has a final code/test/doc disposition.
- No unconditional skip remains merely because it was pre-existing.
- No substantive assertion is silently lost.

## Track B — Restore deterministic file-stream concurrency coverage

### Objective

Replace the skipped `TestFileStreamSemaphore` class with reliable coverage of the actual file-stream concurrency contract.

### Current problem

The Python integration class attempts to hold a file response open by constraining a client socket receive buffer. Kernel TCP buffering can absorb much more data than the application-level socket setting implies, so the test cannot reliably prove that a file-stream permit remains held.

That makes the current external observation mechanism invalid. It does not make the file-stream semaphore contract untestable.

### Required investigation

Inspect the Rust path that:

- plans or opens a file response;
- acquires the `max_file_streams` semaphore permit;
- converts the body into the Hyper response/stream;
- holds the permit for the stream lifetime;
- releases the permit on completion, range completion, client disconnect, and error;
- avoids acquiring a file-stream permit for HEAD responses;
- handles handler-returned in-memory bodies separately.

Identify the narrowest internal seam where body progress can be paused deterministically.

### Preferred test design

Add or extend Rust integration tests using a controllable body/stream fixture with explicit barriers.

A suitable pattern is:

1. Configure `max_file_streams = 1`.
2. Start the first file-body conversion or stream.
3. Block that body at a test-controlled barrier after permit acquisition and before completion.
4. Start a second file-body operation.
5. Assert the second operation has not crossed its acquisition/completion barrier.
6. Release or drop the first operation.
7. Assert the second operation proceeds within a bounded timeout.

Use channels, barriers, notifications, or a test-controlled reader. Do not use file size plus sleep as the primary synchronization mechanism.

### Required invariants to preserve

At minimum, deterministic tests must cover:

- no more than `max_file_streams` active file streams;
- queued file streams proceed after a permit is released;
- normal completion releases the permit;
- dropped/disconnected/erroring streams release the permit;
- completed range responses release the permit;
- HEAD does not consume a file-stream permit;
- in-memory handler bodies do not incorrectly consume this file-stream limit, if that is the intended contract.

The implementation agent may consolidate redundant Python methods into fewer Rust tests, but all listed invariants need mapped coverage.

### Product-code changes

Do not alter production APIs solely to expose a semaphore for tests.

Acceptable minimal techniques include:

- testing a private/internal function in its module test block;
- injecting a test reader through an existing generic/internal boundary;
- creating a test-only helper under `#[cfg(test)]`;
- extracting a small internal permit-lifetime wrapper when this improves production clarity as well as testability.

Do not add public test hooks.

### Python test disposition

After deterministic Rust coverage exists:

- remove the class-level `@unittest.skip`;
- delete Python tests whose only observation mechanism was invalid and whose contract is now covered deterministically in Rust;
- retain Python-level positive smoke tests that remain reliable and add distinct binding/runtime value;
- add comments pointing to the Rust coverage only when useful, not as a substitute for tests.

A valid outcome is that `TestFileStreamSemaphore` no longer exists in Python because its low-level concurrency invariants are covered in Rust and Python retains only stable end-to-end file-serving checks.

### Optional manual qualification

If maintainers still value external TCP backpressure observation, retain it as a manual, nonblocking direct script under `tests/` with an explicit statement that results depend on kernel buffering.

This is optional and must not become a CI gate or evidence artifact.

### Acceptance criteria

- No class-level skip remains for file-stream concurrency.
- The semaphore limit is tested with deterministic synchronization.
- Permit release is covered for completion and abnormal termination.
- HEAD behavior is covered.
- Range behavior is covered.
- The old unreliable socket-buffer assumption is not used as a routine correctness oracle.
- Any removed Python methods have explicit replacement coverage.
- Routine CI remains two jobs.

## Track C — Resolve Python observer delivery deliberately

### Objective

Eliminate observer test skips by aligning product behavior, tests, and documentation.

### Current contradiction

Active project documentation and Python API descriptions state that the Python server can accept an observer and receive structured operational events. Two tests are skipped because events are not delivered.

That is a product/documentation contradiction, not an acceptable permanent skip.

### Required investigation

Trace the observer path end to end:

1. Python `Server(..., observer=...)` constructor storage.
2. Creation and lifetime of `PyLogObserver` or equivalent sink.
3. Logger initialization or observer registration.
4. Interaction with global `OnceLock` logger state.
5. CLI versus embedded-Python initialization behavior.
6. Event emission on startup, request handling, errors, and shutdown.
7. GIL acquisition and exception containment when invoking Python callbacks.
8. Observer lifetime and deregistration across multiple server instances/tests.

Reproduce observer delivery in isolation against the installed wheel before choosing a fix.

### Decision rule

Use this rule:

- If observer callback support is intended and documented, fix event delivery and enable the tests.
- If the API cannot safely or correctly support observers in the current architecture, remove the observer parameter/public claim through an explicit API decision and delete the stale tests.

The preferred outcome is to fix the existing documented functionality unless investigation demonstrates a fundamental incompatibility.

### Fix requirements when retaining observer support

The implementation must ensure:

- the observer remains alive for the server lifetime;
- emitted events reach the Python callback;
- callback invocation acquires the GIL correctly;
- observer exceptions do not crash request handling or shutdown;
- one server's observer does not leak into unrelated server instances;
- multiple tests can create and destroy servers without global initialization panics;
- observer delivery has bounded deterministic test synchronization.

### Test design

Replace arbitrary waiting with `threading.Event` or another explicit signal.

Required tests:

1. **Observer receives events**
   - start a server with a capture observer;
   - perform a request;
   - wait for a specific request-related event or a documented event class;
   - assert the event is a dictionary/structured object with required fields.

2. **Observer failure is contained**
   - observer raises;
   - request handling remains functional according to the documented policy;
   - server can still shut down.

3. **Observer lifetime/isolation**
   - create and close one observed server;
   - create another server with a different/no observer;
   - prove callbacks do not cross instances.

Avoid assertions based merely on startup event count unless startup events are explicitly part of the contract.

### Alternative removal path

If observer support is deliberately removed:

- remove or reject the observer constructor argument;
- update `docs/python-api.md`, architecture docs, skill instructions, and relevant examples;
- remove observer-specific implementation that is otherwise dead;
- delete the stale tests with a clear commit rationale;
- do not leave a no-op accepted argument.

Because this is a public-surface change, use the removal path only when fixing delivery is demonstrably unsafe or disproportionate.

### Acceptance criteria

One of these mutually exclusive outcomes is complete:

**Retained support**

- observer delivery works against the installed wheel;
- both formerly skipped tests are enabled or replaced by stronger deterministic tests;
- observer exceptions are contained;
- lifecycle/isolation is tested;
- active docs accurately describe delivered events and limitations.

**Removed support**

- observer is no longer accepted or advertised as working;
- dead observer plumbing is removed where appropriate;
- stale tests are deleted with a documented contract reason;
- no active doc or example claims observer support.

In either outcome, no observer test remains unconditionally skipped.

## Track D — Audit every remaining Python skip

### Objective

Prevent closure from focusing only on the two known skip sites while leaving other unconditional skips active.

### Required search

```sh
rg -n "@unittest\.skip|@unittest\.skipIf|@unittest\.skipUnless|@unittest\.expectedFailure" \
  crates/eggserve-python/tests
```

Classify every result.

### Native availability guards

Tests currently use patterns such as:

```python
try:
    from eggserve._native import ...
    NATIVE_AVAILABLE = True
except ImportError:
    NATIVE_AVAILABLE = False

@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
```

Under `scripts/test-python-wheel.sh`, native importability is a required smoke check. Therefore these guards provide little value in the supported installed-wheel suite and can hide import regressions if test ordering changes.

Preferred correction:

- import required native symbols directly;
- let import failure fail discovery;
- remove `NATIVE_AVAILABLE` skip guards from installed-wheel tests.

Retain a conditional import only where a file is intentionally runnable in a source-only developer context, and document why that secondary mode matters. The canonical helper must never convert a missing native module into skipped coverage.

### Platform-specific skips

Platform-specific tests may use narrow platform guards when the behavior genuinely cannot run elsewhere. They must:

- identify the target platform explicitly;
- run in the applicable manual platform verification;
- not be used to claim cross-platform coverage from Linux;
- not suppress a Linux-supported behavior.

### Acceptance criteria

- No missing native extension can produce a green suite via skips.
- Every remaining conditional skip has a real platform/environment condition and a documented execution path.
- There are no unconditional skips for supported Linux/Python behavior.
- The final test summary does not contain unexplained skipped tests.

## Track E — Tighten the installed-wheel helper environment

### Objective

Make the shared helper self-contained and equally strict in CI and local `verify.sh full`.

### Required changes to `scripts/test-python-wheel.sh`

At the beginning of the helper:

```sh
export PYTHONNOUSERSITE=1
unset PYTHONPATH
```

Run wheel smoke and tests with the virtual-environment interpreter and the same sanitized environment.

Do not rely on `.github/workflows/ci.yml` to provide environment isolation that local full verification lacks.

### Python selection

Keep one explicit interpreter variable:

```sh
PYTHON="${PYTHON:-python3.14}"
```

Validate the supported range precisely:

```python
(3, 14) <= sys.version_info < (3, 15)
```

Do not accept 3.15+ merely because it is numerically greater than 3.14; package metadata currently declares `<3.15`.

### Maturin selection

Prefer invoking Maturin through the selected interpreter when possible:

```sh
"$PYTHON" -m maturin --version
"$PYTHON" -m maturin build ...
```

This avoids a mismatch between the chosen interpreter environment and a different `maturin` executable on `PATH`.

CI may install Maturin into the setup Python environment before calling the helper.

### Virtual environment portability

Routine closure is Linux-based, but avoid misleading partial Windows support in the helper.

Either:

- implement a small platform-aware virtual-environment interpreter path (`bin/python` versus `Scripts/python.exe`), or
- state clearly that this helper is the Linux/macOS installed-wheel harness and use documented direct commands for manual Windows validation.

Do not keep Windows staging branches while failing later on an unconditional Unix venv path without explanation.

### Cleanup

Ensure cleanup removes only generated content and preserves pre-existing package files.

Before staging the binary, detect whether `python/eggserve/bin` already exists. If it contains tracked or pre-existing files, do not delete the entire directory blindly during cleanup.

Preferred pattern:

- create the target directory;
- record whether the staged binary existed and, if so, preserve/restore it;
- otherwise remove only the binary created by the helper;
- remove the directory only if the helper created it and it is empty.

Continue removing helper-created temporary venvs, wheel output, `__pycache__`, and `.pyc` files.

### Required changes to `scripts/verify.sh`

Do not gate on a generic `python3` alias before invoking the helper.

Either:

```sh
run env PYTHON="${PYTHON:-python3.14}" bash "$SCRIPT_DIR/test-python-wheel.sh"
```

and let the helper report prerequisites, or check the same explicit interpreter variable that the helper uses.

Do not duplicate version and Maturin policy in two scripts unless the checks are exactly shared.

### Acceptance criteria

- The helper sanitizes user-site and `PYTHONPATH` itself.
- The helper enforces Python `>=3.14,<3.15`.
- The helper uses the selected interpreter consistently.
- Local and CI behavior do not depend on different ambient environment variables.
- `verify.sh full` does not require an irrelevant `python3` alias.
- Cleanup does not delete pre-existing package content.
- Successful and failed helper runs leave `git status --short` unchanged.

## Track F — Remove active profile-promotion terminology

### Objective

Describe platform safety directly without relying on the deleted support-profile promotion apparatus.

### Required active-document search

```sh
rg -n "profile promotion|profile decision|promot(e|ion)|support-profiles|candidate profile" \
  AGENTS.md .opencode README.md docs tests scripts .github
```

Historical plans and explicitly historical documents may retain historical language. Active instructions and current support claims may not.

### Windows wording

Replace wording such as:

```text
Independent safety review and profile promotion decision awaited.
```

with direct status language such as:

```text
Windows support is functional and uses handle-relative confinement, but independent adversarial review is incomplete. Do not use Windows builds to serve untrusted public content until that review is completed.
```

This preserves the security limitation without requiring a machine-managed promotion state.

### Production profile headings

Human-readable deployment patterns such as `unix-reverse-proxy` may remain if they are useful descriptive configurations.

What must be removed is any implication that:

- a TOML profile registry controls support;
- a profile must be machine-promoted;
- candidate status is advanced through evidence state;
- CI decides production support status.

Rename headings or tables when needed:

- `Production Profiles` → `Deployment Patterns` or `Deployment Status`;
- `candidate` → direct support/qualification wording;
- `promotion` → explicit outstanding review or test.

### Plan status honesty

Until all Plan 093 closure criteria pass, active docs must say:

```text
Plans 091–092 simplification is implemented; Plan 093 closes remaining skipped-test and verification gaps.
```

Only after final local and CI verification may they say Plans 000–093 are implementation-complete.

### Acceptance criteria

- Active docs contain no machine profile-promotion dependency.
- Windows risk wording remains conservative and direct.
- Human-readable deployment patterns remain only where useful.
- CI is not described as determining production support status.
- Plan status is not advanced before final verification.

## Track G — Establish repeatability for corrected tests

### Objective

Complete the repeated-run requirements that were not established during Plan 092 and add repeatability for the newly corrected skip areas.

### Test execution strategy

Do not rebuild the wheel for every repetition.

Use one working temporary virtual environment created by the helper steps, then invoke the installed-wheel interpreter repeatedly. The implementing agent may add a minimal `--test` selector to `scripts/test-python-wheel.sh` only if it remains simple and generally useful.

Do not add a permanent repetition framework.

### Required repeated runs

#### Shutdown deadline

Run the corrected shutdown-deadline test 20 consecutive times.

Representative command after creating the wheel test venv:

```sh
for i in $(seq 1 20); do
  "$VENV_PYTHON" -m unittest \
    test_server_integration.TestCallbackContainment.test_shutdown_respects_deadline_with_blocked_handler \
    -v
done
```

Adjust module import path to the final test layout and discovery setup.

#### Request body

Run the full request-body module 10 consecutive times:

```sh
for i in $(seq 1 10); do
  "$VENV_PYTHON" -m unittest test_body_primitives -v
done
```

#### Observer

When observer support is retained, run the observer module/tests 20 consecutive times to catch global logger/lifetime leakage.

#### File-stream semaphore

Run the deterministic Rust test target 20 consecutive times:

```sh
for i in $(seq 1 20); do
  cargo test -p eggserve-core <final-file-stream-test-selector> -- --exact
done
```

Use the exact final test selector. If multiple related tests are in one integration target, run the target repeatedly.

### Failure handling

Any intermittent failure is a closure blocker. Do not address it by:

- adding retries to CI;
- increasing sleeps blindly;
- increasing global job timeout;
- weakening timing assertions until they can never fail.

Diagnose the synchronization or product lifetime issue.

### Acceptance criteria

- Shutdown test passes 20/20.
- Request-body module passes 10/10.
- Retained observer tests pass 20/20.
- Deterministic file-stream tests pass 20/20.
- No run hangs beyond its bounded watchdog.
- No flaky-test retry mechanism is added.

## Track H — Final local verification

### Objective

Run the complete supported local verification path on the exact final implementation tree.

### Clean-tree precondition

```sh
git status --short
```

Output must be empty.

### Required commands

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Then:

```sh
git status --short
```

Output must still be empty.

Run applicable focused checks:

```sh
# No routine CI suppression
rg -n "continue-on-error" .github/workflows/ci.yml

# No unconditional supported-behavior skips
rg -n "@unittest\.skip|@unittest\.expectedFailure" crates/eggserve-python/tests

# Conditional skip inventory must be reviewed manually
rg -n "@unittest\.skipIf|@unittest\.skipUnless" crates/eggserve-python/tests

# No active deleted release/evidence machinery
rg -n "release/criteria\.toml|ci-gate-evidence|release_criteria|evidence-aggregate" \
  .github scripts AGENTS.md .opencode README.md docs

# No active profile-promotion language
rg -n "profile promotion|profile decision|support-profiles" \
  AGENTS.md .opencode README.md docs tests scripts .github
```

### Test-count review

Record the final test summary in the implementation handoff.

Any skipped tests must be listed individually with their real platform/environment condition. A summary such as “18 skipped, documented” is insufficient.

### Acceptance criteria

- `verify.sh fast` passes.
- `verify.sh full` passes.
- Working tree is clean before and after.
- No supported Python behavior is unconditionally skipped.
- No active stale evidence/profile terminology remains.
- Final test summary has an explicit explanation for every remaining conditional skip.

## Track I — Final same-commit CI verification

### Objective

Establish that the final implementation commit—not an earlier intermediate commit—passes both blocking routine jobs.

### Required workflow state

The final `.github/workflows/ci.yml` must still have:

- one workflow;
- triggers for pull requests and pushes to `main`;
- exactly `rust` and `python` jobs;
- Ubuntu runners;
- no job-level or required-step `continue-on-error`;
- no OS matrix;
- no evidence upload/aggregation;
- no release or publication commands.

### Final run

Push the final implementation commit and verify in GitHub Actions:

- workflow parses and starts;
- `rust` succeeds;
- `python` succeeds;
- neither job is neutral, skipped, or allowed-failure;
- no third routine job appears;
- no release workflow runs;
- direct logs contain the Python test summary.

If any code, test, workflow, helper, or active-document change occurs after that run, rerun CI on the newer commit.

### Handoff record

Do not add a committed exact-SHA evidence system.

Record the final commit SHA, workflow run URL, job conclusions, local command results, repeated-run counts, and skip inventory in one of:

- the implementing pull request description/comment;
- the final commit/merge handoff message;
- the agent's final implementation report.

This is a human handoff record, not a permanent repository gate.

### Connector limitation

Some GitHub connector methods expose only pull-request-triggered workflow runs. A missing connector result is not proof that CI did not run. The implementer must inspect the actual Actions run through an available API/UI and provide the run URL in the handoff.

### Acceptance criteria

- Both routine jobs pass on the final commit.
- Python is fully blocking.
- The final run URL is provided in the handoff.
- No later repository change invalidates the same-commit result.
- No evidence framework is added.

## Track J — Final documentation and plan-status closure

### Objective

Advance plan status only after technical closure is established.

### Before verification completes

Active status must indicate Plan 093 is in progress/pending closure.

### After all criteria pass

Update:

- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- any other active project-status statement.

Final wording may state:

```text
Plans 000–093 are implementation-complete. Plan 091 defines the current simplified CI and manual release policy; Plans 092–093 closed the Python installed-wheel and test-reliability gaps.
```

Do not edit historical plan files to mark checkbox state or embed final SHAs.

### Acceptance criteria

- Status is advanced only after final local and CI success.
- Active docs match actual behavior.
- Historical plans remain historical records.
- No exact-SHA closure churn is introduced.

## Ordered implementation sequence

Use small, reviewable commits. A recommended sequence follows.

### Commit 1 — Skip inventory and helper hygiene

- inspect and classify every skip;
- sanitize `PYTHONNOUSERSITE`/`PYTHONPATH` in the helper;
- align Python version and Maturin invocation;
- fix `verify.sh` interpreter prerequisite mismatch;
- make cleanup preserve pre-existing staged package content.

Acceptance:

- helper is self-contained;
- `verify.sh full` invokes the same interpreter policy;
- no coverage disposition occurs silently.

### Commit 2 — Deterministic file-stream coverage

- add deterministic Rust tests for file-stream permit lifetime;
- remove the skipped Python class;
- retain only reliable Python-level file serving tests;
- map every removed Python invariant to replacement coverage.

Acceptance:

- no file-stream class skip remains;
- required invariants have deterministic tests;
- tests pass repeatedly.

### Commit 3 — Observer contract closure

- reproduce observer delivery failure;
- fix observer delivery and tests, or deliberately remove unsupported observer API/docs;
- remove observer skips;
- add lifecycle/isolation coverage if support remains.

Acceptance:

- observer code/tests/docs agree;
- no observer skip remains;
- repeated tests pass.

### Commit 4 — Remaining skip and terminology cleanup

- remove native-availability skips that can hide installed-wheel failures;
- review legitimate platform guards;
- remove active profile-promotion terminology;
- set plan status to pending Plan 093 closure, not complete.

Acceptance:

- no supported Linux behavior is skipped;
- active platform wording is direct and conservative.

### Commit 5 — Local repeatability and full verification fixes

- run required repetition loops;
- correct any remaining nondeterminism;
- run `verify.sh fast` and `full` from a clean tree;
- verify clean tree afterward.

Acceptance:

- all repeated-run thresholds pass;
- local full verification passes cleanly.

### Commit 6 — Final status and same-commit CI

- push final implementation tree;
- obtain successful `rust` and `python` jobs;
- after success, update active status to Plans 000–093 complete if that status update itself will receive a successful CI run;
- otherwise include status update in the final implementation commit before its CI run;
- provide run URL and final results in handoff.

Acceptance:

- final status commit itself is green;
- all Definition of Done items are true.

## Rejection criteria

Reject the implementation if any of these are true:

- `TestFileStreamSemaphore` remains skipped;
- observer tests remain skipped while observer support is still advertised;
- a skip is replaced with an always-active environment conditional;
- missing native bindings can produce skipped tests instead of failure;
- file-stream concurrency coverage is simply deleted;
- replacement tests use arbitrary sleeps as their primary oracle;
- file-stream tests still depend on socket receive-buffer sizing as the routine gate;
- observer support accepts callbacks but silently drops all events;
- production APIs gain test-only hooks;
- `scripts/test-python-wheel.sh` and `verify.sh` diverge again;
- local full verification depends on CI-only environment variables;
- helper cleanup deletes pre-existing package content;
- Python 3.15+ is accepted despite package metadata `<3.15`;
- profile-promotion terminology remains in active support instructions;
- plan status says complete before final green CI;
- repeated-run requirements are omitted;
- a flaky-test retry action is added;
- routine CI gains more jobs or an OS matrix;
- release/evidence automation returns;
- publication occurs as part of this work.

## Definition of Done

Plan 093 is complete only when every item below is true.

### Skip closure

- [ ] Every Python skip has been inventoried.
- [ ] No unconditional skip remains for supported Linux/Python behavior.
- [ ] No missing-native condition can turn a broken wheel into a green skipped suite.
- [ ] Every remaining conditional skip has a real platform/environment reason.
- [ ] Final handoff lists every remaining skip individually.

### File-stream concurrency

- [ ] The skipped `TestFileStreamSemaphore` class is removed or fully enabled.
- [ ] Maximum active file streams are tested deterministically.
- [ ] Queued stream release is tested deterministically.
- [ ] Normal completion releases permits.
- [ ] Disconnect/drop/error releases permits.
- [ ] Range completion releases permits.
- [ ] HEAD permit behavior is tested.
- [ ] In-memory handler body behavior is covered according to contract.
- [ ] Replacement tests pass 20 consecutive runs.

### Observer behavior

- [ ] Observer behavior has an explicit retain-or-remove decision.
- [ ] If retained, observer events are delivered against the installed wheel.
- [ ] If retained, observer callback exceptions are contained.
- [ ] If retained, observer lifetime/isolation is tested.
- [ ] If removed, API and active documentation no longer advertise observer support.
- [ ] No observer test remains skipped.
- [ ] Retained observer tests pass 20 consecutive runs.

### Installed-wheel helper

- [ ] `scripts/test-python-wheel.sh` exports `PYTHONNOUSERSITE=1`.
- [ ] The helper neutralizes ambient `PYTHONPATH`.
- [ ] Python support is enforced as `>=3.14,<3.15`.
- [ ] The selected Python interpreter is used consistently for Maturin and venv creation.
- [ ] Import-boundary assertions still prove site-packages use.
- [ ] CLI smoke checks remain blocking.
- [ ] Cleanup preserves pre-existing package content.
- [ ] Failed and successful runs leave the working tree unchanged.

### Local verification

- [ ] `verify.sh fast` remains Rust-only.
- [ ] `verify.sh full` calls the shared wheel helper.
- [ ] `verify.sh full` does not require an irrelevant `python3` alias.
- [ ] `verify.sh full` fails clearly when required tools are missing.
- [ ] Shutdown-deadline test passes 20/20.
- [ ] Request-body module passes 10/10.
- [ ] `verify.sh fast` passes on the final tree.
- [ ] `verify.sh full` passes on the final tree.
- [ ] `git status --short` is empty before and after full verification.

### CI

- [ ] One routine CI workflow exists.
- [ ] Exactly two jobs exist: `rust` and `python`.
- [ ] Both jobs use Ubuntu.
- [ ] Both jobs are blocking.
- [ ] No required step uses `continue-on-error`.
- [ ] Jobs run concurrently.
- [ ] Python calls the shared installed-wheel helper.
- [ ] No matrix, evidence aggregation, release, publication, benchmark, soak, or proxy setup returns.
- [ ] Both jobs pass on the final implementation/status commit.
- [ ] Final Actions run URL is provided in the handoff.

### Documentation

- [ ] Active docs contain no machine profile-promotion dependency.
- [ ] Windows status is direct, conservative, and accurate.
- [ ] Deployment patterns are not represented as machine-promoted evidence states.
- [ ] Active docs accurately describe observer support or removal.
- [ ] Active docs accurately describe routine versus manual platform testing.
- [ ] Plan status is advanced only after final verification.

### Plan 091–092 final closure

- [ ] Plan 091's simplified two-job CI remains intact.
- [ ] Plan 091's manual release policy remains intact.
- [ ] Plan 092's installed-wheel boundary remains intact.
- [ ] Plan 092's Python blocking requirement remains intact.
- [ ] Plan 092's no-suppressed-known-failures requirement is now satisfied.
- [ ] Plan 092's repeated-run requirements are satisfied.
- [ ] Plan 092's same-final-commit CI requirement is satisfied.
- [ ] Release/evidence machinery remains deleted.

## Final handoff template

The implementing agent should provide a concise final report using this shape:

```text
Plan 093 final commit: <sha>
GitHub Actions run: <url>
Rust job: success
Python job: success

Local verification:
- verify.sh fast: pass
- verify.sh full: pass
- clean tree before/after: yes

Repeated runs:
- shutdown deadline: 20/20
- request-body module: 10/10
- file-stream deterministic target: 20/20
- observer target (if retained): 20/20

Skip inventory:
- <remaining conditional skip>: <exact platform/environment reason and execution path>
- or: none

Observer disposition:
- retained and fixed / removed from contract

File-stream disposition:
- deterministic Rust coverage added; invalid external Python tests removed/replaced
```

Do not turn this report into a generated repository artifact or recurring evidence requirement.

## Handoff note

The repository is already close to the intended final shape. The remaining problem is not insufficient CI ceremony. It is the use of skipped tests to conceal newly exposed behavior gaps.

The correct closure preserves the small two-job workflow and resolves coverage at the appropriate layer:

1. deterministic Rust tests for low-level permit lifetime;
2. installed-wheel Python tests for public binding/runtime behavior;
3. direct documentation for platform limitations;
4. one clean same-commit verification result.

When choosing between alternatives, prefer the outcome that fixes or deliberately reconciles the contract, keeps Python blocking, removes unconditional skips, preserves substantive coverage, and adds no new administrative framework.