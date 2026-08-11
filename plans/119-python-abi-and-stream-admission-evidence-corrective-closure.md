# Plan 119 — Python ABI and Stream-Admission Evidence Corrective Closure

## Status

**COMPLETE — 2026-08-11.**

This is a single narrow corrective pass after the Plan 112–118 consolidation roadmap was marked complete.

Plan 118 explicitly rejected creating another plan for minor prose preferences. This plan is nevertheless required because the post-closure review found two substantive evidence defects rather than stylistic drift:

1. the Python package advertises CPython 3.11+ stable-ABI compatibility, but the PyO3 manifest enables bare `abi3` while routine wheel CI builds with CPython 3.14; bare `abi3` selects the host interpreter's stable-ABI floor rather than pinning the declared Python 3.11 minimum;
2. three file-stream permit tests in `streaming_buffer_qualification.rs` operate at the service layer with a separately constructed `RuntimeState`, so they cannot prove ownership or release of the production runtime's file-stream semaphore. Two are already ignored; one still passes without exercising the claimed invariant.

A third cleanup item is directly tied to Plan 115 closure:

3. `plans/plan115-inventory.md` identifies itself as a temporary working inventory that must be deleted when Plan 115 closes, but it remains in the repository.

No new product feature, architecture track, CI matrix, release automation, or filesystem/HTTP hardening work is authorized.

Baseline for this plan:

```text
main = 9b609cf84a87ab574a0e565c9f6d6fe165eab243
```

---

## Goal

Restore truthful closure for Plans 112–118 with the smallest implementation that proves the contracts already claimed:

```text
Python distribution
    -> CPython >=3.11 metadata
    -> PyO3 explicitly targets the Python 3.11 stable ABI
    -> release-equivalent wheel tag advertises that floor
    -> the same wheel imports and passes representative/full installed tests
       on CPython 3.11 and the current CPython 3.14 build host

file-stream admission
    -> production RuntimeState owns the permit pool
    -> live runtime saturation still maps to 503
    -> disconnecting/dropping an active file-stream transport releases its permit
    -> no service-layer test claims to prove a runtime-owned semaphore it cannot reach

verification/planning hygiene
    -> temporary Plan 115 inventory is removed
    -> no ignored or false-positive admission test remains solely to preserve test count
    -> routine CI stays small
```

This should be the final corrective pass for the Plan 112 consolidation track.

---

## Product and process constraints

Preserve all of the following:

- EggServe remains a hardened static HTTP/1.1 server with reusable HTTP/security primitives and an `http.server`-shaped Python facade.
- No HTTP client subsystem is restored.
- No deprecated pre-runtime service adapter is restored.
- Static filesystem confinement behavior is unchanged.
- Runtime-owned `max_file_streams` admission remains the single production authority.
- Standalone Rust TLS remains optional.
- Python wheels continue to include TLS so `HTTPSServer` / `ThreadingHTTPSServer` remain consistently available.
- Routine GitHub CI remains two jobs (`rust`, `python`).
- GitHub Actions does not publish releases.
- No Python-version matrix is added to routine CI merely to prove abi3.
- Expensive diagnostics remain manual/targeted.
- Plans 000–118 remain historical records; do not rewrite their historical implementation narrative.

---

# Track A — Re-establish the exact baseline before editing

Inspect current state at minimum:

```text
crates/eggserve-python/Cargo.toml
crates/eggserve-python/pyproject.toml
scripts/test-python-wheel.sh
scripts/verify.sh
.github/workflows/ci.yml
README.md
SECURITY.md
docs/python-packaging.md
docs/toolchain-support.md
architecture/eggserve-python.md
plans/117-python-distribution-and-compatibility-cleanup.md
plans/118-documentation-consolidation-and-roadmap-closure.md

crates/eggserve-core/src/server/connection.rs
crates/eggserve-core/src/server/state.rs or equivalent RuntimeState definition
crates/eggserve-core/tests/streaming_buffer_qualification.rs
crates/eggserve-core/tests/server_integration.rs
crates/eggserve-core/tests/http_wire_correctness.rs
plans/plan115-inventory.md
```

Record the actual facts before modification:

```text
Python metadata floor           = >=3.11
current PyO3 feature            = abi3 (bare)
routine Python CI interpreter   = 3.14
standalone wheel test default   = python3.14
runtime stream permit owner     = RuntimeState
transport conversion            = uses RuntimeState file-stream semaphore
current production saturation test
                               = runtime_file_admission_is_shared_across_connections
current custom-response saturation test
                               = custom_service_file_stream_saturation_maps_503_and_recovers
invalid service-layer tests     = client_disconnect_releases_stream_permits
                                 forced_shutdown_releases_stream_permits
                                 concurrent_stream_exhaustion_returns_503
```

Confirm these names against current source rather than blindly relying on this plan if the implementation moved after plan creation.

### Acceptance criteria

- implementation begins from current `main`, not an older Plan 118 tree;
- bare `abi3` plus a 3.14 build host is confirmed before changing the manifest;
- the three service-layer permit tests are classified by what semaphore they actually touch;
- existing production-boundary admission tests are identified before any test deletion;
- no unrelated failing test or feature is pulled into this pass without direct causal evidence.

### Stop condition

If current `main` has already independently corrected one of these defects, do not reimplement it. Verify it against the acceptance criteria below and limit the diff accordingly.

---

# Track B — Pin the Python stable-ABI floor to CPython 3.11

## B1 — Correct the PyO3 feature declaration

The package currently claims:

```toml
requires-python = ">=3.11"
```

Therefore the extension build must explicitly target the Python 3.11 stable ABI rather than deriving its minimum from whichever host interpreter happens to build the wheel.

Change the PyO3 dependency from the equivalent of:

```toml
pyo3 = { version = "0.24", features = ["extension-module", "abi3"] }
```

to the version-pinned stable ABI feature supported by the current PyO3 release:

```toml
pyo3 = { version = "0.24", features = ["extension-module", "abi3-py311"] }
```

Do not change the minimum Python version unless a concrete compatibility failure demonstrates that 3.11 cannot support the current extension API.

If PyO3 0.24 unexpectedly cannot build the existing code with `abi3-py311`, first determine whether the blocker is a real stable-ABI API limitation or only build-host detection. Do not immediately upgrade PyO3 or rewrite the Python API. A dependency upgrade is authorized only if it is the smallest safe correction and all existing behavior remains compatible.

## B2 — Preserve CPython 3.14 build-host compatibility

The current repository uses:

```text
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
```

because PyO3 0.24 predates official CPython 3.14 build-host support.

Retain this environment override only where still required to build with the routine 3.14 host. Do not present it as the mechanism that establishes the Python 3.11 ABI floor; the `abi3-py311` feature owns that contract.

## B3 — Prove the wheel tag

Build a release-equivalent wheel on the same OS/architecture using CPython 3.14 as the build interpreter after the manifest correction.

The produced wheel must advertise an abi3 tag whose minimum CPython version is 3.11. For the normal GIL-enabled CPython ABI this should be equivalent to:

```text
cp311-abi3-<platform>.whl
```

Do not accept:

```text
cp314-abi3-<platform>.whl
```

as evidence for the advertised `>=3.11` contract.

The exact platform suffix may vary by OS, architecture, and wheel-repair tooling; only the Python/ABI portion is normative here.

### Acceptance criteria

- PyO3 uses `abi3-py311` (or an equivalent explicit 3.11 minimum supported by the exact PyO3 version);
- `requires-python = ">=3.11"` remains truthful;
- the release-equivalent wheel built from a 3.14 host is tagged for Python 3.11 abi3 rather than Python 3.14 abi3;
- no second Python package or feature-specific wheel is introduced;
- no Python API functionality is removed to obtain the stable ABI;
- `HTTPSServer` and `ThreadingHTTPSServer` remain available;
- the bundled native CLI remains present in the wheel.

### Rejection conditions

Reject this track if it:

- leaves bare `abi3` while claiming the build is independent of the host interpreter's ABI floor;
- changes only `pyproject.toml` metadata without correcting the compiled extension ABI;
- adds a permanent multi-version build matrix to routine CI;
- drops HTTPS classes or splits TLS/non-TLS Python distributions;
- upgrades PyO3 solely for novelty when the pinned feature works on 0.24;
- claims Python 3.11 support from wheel filename inspection alone without installation/runtime verification.

---

# Track C — Verify the same wheel on CPython 3.11 and 3.14

## C1 — Build once, install twice

Stable-ABI evidence is strongest when the same release-equivalent wheel is tested under both the minimum and current build-host interpreters.

On a host where CPython 3.11 and 3.14 are available:

1. build the default `dist` CLI;
2. stage the platform-native CLI into the Python package as normal;
3. build one `abi3-py311` wheel with CPython 3.14 / maturin;
4. preserve that exact wheel long enough for both installation tests;
5. create isolated CPython 3.11 and CPython 3.14 virtual environments;
6. install the same wheel file into each environment;
7. run installed-package smoke and compatibility tests under both;
8. remove temporary staging/artifacts afterward.

Do not permanently change `scripts/test-python-wheel.sh` merely to retain a temporary artifact if the above can be performed as a one-time release-equivalent verification procedure.

If a tiny reusable option to preserve/reuse a wheel is demonstrably simpler than duplicating fragile shell logic, it may be added, but it must remain optional and must not turn the harness into a packaging framework.

## C2 — Minimum installed verification

For each of CPython 3.11 and 3.14, prove at minimum:

```text
import eggserve
import eggserve._native
import eggserve.server

python -m eggserve --help
bundled CLI discovery succeeds
HTTPServer / ThreadingHTTPServer basic live socket serving succeeds
SimpleHTTPRequestHandler secure static path succeeds
HTTPSServer smoke/compatibility succeeds
removed client types remain absent
```

Prefer running the existing installed-wheel Python test suite under both interpreters if it is not materially expensive. Do not create a second test suite that duplicates the existing one.

## C3 — Keep routine CI narrow

Routine CI may continue to use CPython 3.14 only. The purpose of abi3 is specifically to avoid a permanent interpreter matrix.

Record minimum/current compatibility as release-time or corrective-pass evidence. Future releases should verify the minimum supported interpreter at least when:

- the PyO3 version changes;
- the minimum Python version changes;
- native bindings add new Python C-API usage;
- the stable-ABI feature configuration changes.

Do not require every ordinary PR to retest every supported interpreter.

### Acceptance criteria

- one exact wheel artifact is installed successfully under CPython 3.11 and CPython 3.14 on the verification host;
- native extension import succeeds under both;
- bundled CLI remains available under both;
- supported Python server facade passes representative or full installed-wheel tests under both;
- HTTPS compatibility remains functional;
- no source-tree import is accepted as a substitute for installed-wheel proof;
- routine CI remains one Python job rather than becoming a version matrix;
- closure metadata records the wheel filename/tag and interpreter versions actually tested.

### Environment constraint

If the implementation environment genuinely cannot provide CPython 3.11 and 3.14 simultaneously, do not falsely mark this criterion complete. Use existing hosted CI/manual infrastructure if available, or leave the compatibility evidence explicitly pending until the minimum interpreter can run.

Do not weaken the acceptance criterion merely to close the plan.

---

# Track D — Reconcile Python version and verification wording

After the ABI correction, audit active claims using searches equivalent to:

```sh
rg -n "abi3|abi3-py|CPython 3\.11|CPython 3\.14|Python 3\.11|Python 3\.14|>=3\.11|tested.*3\.11" \
  README.md SECURITY.md crates/eggserve-python scripts docs architecture AGENTS.md .opencode
```

## D1 — `scripts/verify.sh`

The script may continue to default to:

```sh
PYTHON=${PYTHON:-python3.14}
```

because 3.14 is the routine build interpreter.

However, error/help text must not state that Python 3.14 is the package requirement if the package supports 3.11+.

Preferred wording should distinguish:

```text
selected verification interpreter
package minimum supported interpreter
routine CI build interpreter
```

For example, `full` may require the configured `$PYTHON` plus maturin, while metadata independently states `>=3.11`.

## D2 — `scripts/test-python-wheel.sh`

Keep the current `PYTHON` override so implementation/release verification can run:

```sh
PYTHON=python3.11 bash scripts/test-python-wheel.sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Its prerequisite check should continue to reject versions below 3.11.

Do not hardwire the harness to one interpreter except for the default executable choice.

## D3 — Documentation claims

Correct active documentation so it distinguishes three different facts:

1. **Declared support:** CPython 3.11+ GIL-enabled builds via abi3.
2. **Routine CI:** Linux wheel is built/tested with CPython 3.14 unless the workflow changes.
3. **Release/platform qualification:** macOS/Windows and minimum-version checks are manual unless there is concrete automation proving otherwise.

In particular, avoid broad wording such as:

```text
"the release wheel is tested for CPython 3.11+ on Linux, macOS, and Windows"
```

unless release evidence actually demonstrates every part of that statement.

Prefer precise wording such as:

```text
The wheel targets the CPython 3.11 stable ABI and supports GIL-enabled CPython
3.11+. Routine CI verifies the Linux wheel with CPython 3.14; release/platform
qualification is performed manually as documented.
```

Adapt to the actual evidence rather than copying this sentence mechanically.

### Acceptance criteria

- no active document conflates package minimum support with routine CI interpreter;
- no active document claims Windows/macOS/version coverage not supported by actual release evidence;
- `verify.sh full` no longer says CPython 3.14 is the package-wide requirement;
- `test-python-wheel.sh` remains configurable through `PYTHON`;
- PyPy/free-threaded claims remain unchanged unless separately verified;
- manual release policy remains unchanged.

---

# Track E — Replace invalid stream-permit tests with production-boundary evidence

## E1 — Remove tests that cannot observe the claimed runtime pool

The current `streaming_buffer_qualification.rs` contains service-layer tests that construct an independent `RuntimeState` and then call `StaticService::call()` directly.

That shape cannot prove that a response obtained a permit from the constructed runtime semaphore because file-stream admission occurs at the runtime transport conversion boundary.

The following tests must not remain as current evidence in that form:

```text
client_disconnect_releases_stream_permits
forced_shutdown_releases_stream_permits
concurrent_stream_exhaustion_returns_503
```

Disposition:

### `concurrent_stream_exhaustion_returns_503`

Delete the invalid service-layer test if `server_integration.rs` continues to prove the same contract through a running server, specifically the equivalent of:

```text
runtime_file_admission_is_shared_across_connections
```

That production-boundary test is the authoritative saturation/503 evidence.

### `forced_shutdown_releases_stream_permits`

Delete unless a user-visible invariant remains that is not covered elsewhere.

Destroying the running server also destroys its runtime-owned admission pool; asserting that a separately constructed semaphore regains permits after unrelated response values are dropped is not meaningful production evidence.

Do not replace this test with another synthetic ownership test solely to preserve test count.

### `client_disconnect_releases_stream_permits`

This invariant is meaningful and should be preserved, but it must be tested at the actual runtime boundary.

Replace it with a live-socket/server test, preferably in `server_integration.rs` or the most appropriate existing runtime integration suite.

Recommended test shape:

```text
RuntimeConfig.max_file_streams = 1
start real Server with a static or custom file response large enough to remain active
connect client A
send GET
read response headers and enough body to prove 200/file streaming began
keep stream active or close client A before full body completion
while A owns permit, prove concurrent file request B receives 503 if timing can be deterministic
close/drop client A
within a bounded retry/deadline, issue file request C
prove C receives 200 and valid body
shutdown server cleanly
```

The test must prove permit ownership/release through the same `RuntimeState` used by the running server. Do not expose new production internals solely to make the test easy.

If existing `runtime_file_admission_is_shared_across_connections` can be safely extended to prove recovery after client A disconnects without becoming timing-fragile, prefer extending it rather than adding another near-duplicate test.

## E2 — Avoid timing-based flakiness

A large file alone is not a deterministic synchronization primitive.

Prefer one of these approaches, in order:

1. use observable wire state (first response headers received while body remains unread) plus bounded retry for post-disconnect recovery;
2. use an existing controlled custom file-body mechanism if it represents the actual transport boundary;
3. add test-only synchronization at the service/test layer only if it does not leak into production API.

Do not add sleeps as the sole proof that the permit must be held/released.

## E3 — Preserve unique runtime admission coverage

After cleanup, retain explicit evidence for:

- file-stream saturation maps to 503;
- the one server-owned pool is shared across connections;
- custom file-backed responses use the same pool;
- permit release after an interrupted/disconnected active file stream permits a later request;
- HEAD/byte/empty responses do not consume file-stream permits where already covered.

Do not duplicate every invariant in both `streaming_buffer_qualification.rs` and `server_integration.rs`.

### Acceptance criteria

- no ignored test remains solely because it targets the wrong semaphore layer;
- `client_disconnect_releases_stream_permits` is removed or replaced by a production-boundary equivalent;
- concurrent saturation/503 remains covered by a running-server test;
- client-disconnect permit recovery is covered by a running-server/live-transport test;
- the replacement test fails if runtime permit release is intentionally broken;
- no new public testing hook is added to `RuntimeState` merely for this test;
- total test count may decrease; count preservation is not an acceptance criterion.

### Rejection conditions

Reject this track if it:

- merely un-ignores the old tests without changing their ownership path;
- manipulates an unrelated `RuntimeState` and claims it proves production admission;
- replaces real transport evidence with mocks of semaphore behavior;
- adds broad runtime instrumentation or a new public API solely for testing;
- deletes all disconnect-release coverage;
- hides a discovered runtime bug by weakening assertions.

### Runtime defect stop rule

If the real live-socket disconnect test exposes a production permit leak, stop treating this as test-only cleanup. Fix the smallest runtime defect, add the regression test, and record the behavior change explicitly in Plan 119 closure metadata.

Do not rewrite the test to accommodate an actual leak.

---

# Track F — Remove the temporary Plan 115 working inventory

`plans/plan115-inventory.md` begins with the explicit statement:

```text
Temporary working inventory. Deleted when Plan 115 closes.
```

Plan 115 is complete. Delete this file.

Before deletion, verify that no unique normative information exists only in the inventory. Any current operational fact that still matters should already be owned by:

```text
architecture/testing-and-conformance.md
scripts/verify.sh
docs/release-process.md
AGENTS.md
```

Do not promote the inventory into another permanent registry.

Search for references before deletion:

```sh
rg -n "plan115-inventory|Plan 115.*inventory|verification inventory" .
```

Remove/update only active references that would otherwise break.

### Acceptance criteria

- `plans/plan115-inventory.md` is deleted;
- no broken current link/reference remains;
- no replacement gate registry, test database, or generated inventory is created;
- historical Plan 115 remains sufficient to explain why the temporary inventory existed.

---

# Track G — Verification sequence

Run verification in increasing cost order.

## G1 — Static/scope checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
```

Confirm removed/dead evidence is gone:

```sh
rg -n "client_disconnect_releases_stream_permits|forced_shutdown_releases_stream_permits|concurrent_stream_exhaustion_returns_503" \
  crates/eggserve-core/tests

rg -n "plan115-inventory" .
```

Expected result: no stale service-layer tests and no active inventory reference, except intentional historical text in Plan 119 closure if added.

## G2 — TLS regression

```sh
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
```

## G3 — Runtime admission tests

Run the exact production-boundary tests directly in addition to the workspace pass, so failures are easy to diagnose:

```sh
cargo test -p eggserve-core --test server_integration runtime_file_admission -- --nocapture
```

Adapt the filter to the final test names.

Verify both saturation and disconnect recovery.

## G4 — Installed-wheel verification on the routine host

```sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

This must remain green after `abi3-py311`.

## G5 — Stable-ABI cross-version proof

Build one preserved release-equivalent wheel using CPython 3.14, verify its tag, then install that same artifact into isolated CPython 3.11 and 3.14 environments.

Record:

```text
wheel filename
wheel SHA-256
build interpreter version
minimum test interpreter version
current test interpreter version
maturin version
PyO3 version
platform / architecture
```

Run the installed-wheel smoke/compatibility suite under both interpreters.

Do not substitute two independently built wheels for this particular proof unless the environment makes same-wheel reuse impossible; if so, document the limitation and do not overstate the evidence.

## G6 — Optional release checks

If release tooling is already available:

```sh
bash scripts/verify-cargo-packages.sh --mode core
bash scripts/verify-cargo-packages.sh --mode bin
cargo audit
cargo deny check
```

These remain manual release checks and are not required to become routine CI gates.

### Acceptance criteria

- all routine Rust checks pass;
- TLS regression checks pass;
- production-boundary stream-admission tests pass;
- installed-wheel verification passes on CPython 3.14;
- the same `cp311-abi3` wheel is verified under CPython 3.11 and 3.14 before claiming the corrective pass complete;
- no permanent CI matrix is added;
- no release publication occurs from this plan.

---

# Track H — Documentation and closure metadata

After behavior/evidence is correct, update only active documents whose claims changed or were previously too broad.

Expected candidates:

```text
README.md
SECURITY.md
AGENTS.md
.opencode/skills/eggserve-dev/SKILL.md
architecture/eggserve-python.md
architecture/testing-and-conformance.md
docs/python-packaging.md
docs/toolchain-support.md
docs/release-process.md
plans/117-python-distribution-and-compatibility-cleanup.md
plans/118-documentation-consolidation-and-roadmap-closure.md
plans/119-python-abi-and-stream-admission-evidence-corrective-closure.md
```

Do not edit every candidate automatically. Change only files containing an inaccurate current claim or necessary closure reference.

## H1 — Plan 117 record

Preserve Plan 117 as historical intent/implementation record, but add a concise corrective note if necessary:

```text
Post-closure review found bare abi3 did not pin the advertised 3.11 ABI floor
when building on the routine 3.14 host. Plan 119 corrected the feature to
abi3-py311 and supplied minimum/current installed-wheel evidence.
```

Do not rewrite Plan 117 to pretend the original implementation was already correct.

## H2 — Plan 118 record

Do not erase its original closure metadata. Add a concise post-closure correction note explaining that Plan 119 corrected ABI/evidence and stale qualification-test defects discovered afterward.

After Plan 119 passes, Plan 118 may remain `COMPLETE`, but its closure record must point to Plan 119 so future reviewers do not treat the superseded claims as the final evidence.

## H3 — Plan 119 closure

Mark Plan 119 complete only after all hard acceptance criteria pass.

Record:

- implementation commit SHA;
- exact PyO3 feature used;
- wheel filename/tag and SHA-256;
- CPython 3.11 and 3.14 installed-wheel results;
- runtime admission test names/results;
- deleted invalid/temporary files;
- routine/TLS verification results;
- any environment-limited evidence explicitly.

Do not create Plan 120 for minor prose cleanup after successful closure.

### Acceptance criteria

- active documentation states the exact ABI/support/CI facts;
- historical plans retain truthful correction notes rather than rewritten history;
- no broad platform/version testing claim exceeds recorded evidence;
- Plan 119 closure contains reproducible evidence rather than only “tests pass”;
- Plans 112–118 can again be treated as closed after this corrective pass.

---

# Final acceptance criteria

Plan 119 is complete only when **all** of the following are true.

## Python ABI contract

- `requires-python` remains `>=3.11` unless a concrete verified blocker requires a deliberate contract change;
- PyO3 explicitly targets the CPython 3.11 stable ABI using `abi3-py311` or the exact-version equivalent;
- a release-equivalent wheel built on the CPython 3.14 host advertises a CPython 3.11 abi3 floor;
- the same wheel artifact installs and loads under CPython 3.11 and CPython 3.14;
- installed-wheel server/static/TLS smoke or full compatibility tests pass under both;
- the bundled CLI is present and executable from both installations;
- removed HTTP client types remain absent;
- no routine Python-version CI matrix is introduced.

## Verification truthfulness

- routine CI remains the small two-job workflow;
- routine CI's Python 3.14 interpreter is documented as the routine build/test host, not the package minimum;
- `verify.sh` does not claim Python 3.14 is required by the package as a whole;
- active docs do not claim all platform/version combinations are tested unless evidence exists;
- manual release policy is unchanged.

## File-stream admission evidence

- no service-layer test claims to prove the production runtime's separate semaphore;
- the two currently ignored invalid permit tests are deleted or replaced at the correct boundary;
- the currently false-positive service-layer disconnect test is deleted or replaced;
- a running-server/live-transport test proves permit recovery after client disconnect;
- running-server coverage still proves saturation maps to 503 and the pool is shared across connections;
- custom file-backed responses remain covered by the runtime-owned pool;
- no new public test hook or semaphore API is introduced solely for testing.

## Repository simplification

- `plans/plan115-inventory.md` is deleted as its own header requires;
- no replacement registry/database/generated evidence mechanism is created;
- no HTTP client, legacy service adapter, ASGI/WSGI, HTTP/2/3, release automation, or feature expansion is introduced;
- no filesystem hardening behavior is weakened;
- no additional corrective roadmap is needed.

## Verification

- `cargo fmt --all -- --check` passes;
- workspace clippy passes with `-D warnings`;
- `cargo test --workspace` passes with no newly ignored test used to hide a defect;
- TLS clippy/tests pass;
- installed Python wheel verification passes on the routine host;
- same-wheel CPython 3.11/3.14 abi3 proof passes;
- targeted runtime file-stream admission/disconnect tests pass.

---

# Explicit non-goals

Do not use this pass to:

- add new HTTP features;
- add client functionality;
- redesign the service trait or runtime;
- alter static file semantics unrelated to a discovered permit leak;
- add a response-write timeout;
- change connection-total-timeout semantics;
- redesign error taxonomy again;
- upgrade the Rust edition;
- perform broad dependency upgrades;
- add Python 3.10 or older support;
- add PyPy or free-threaded CPython support;
- add Windows/macOS CI matrices;
- add automated PyPI/crates.io/GitHub release workflows;
- make cargo audit/deny routine CI gates;
- create performance or artifact-size gates;
- add a new verification framework;
- preserve invalid tests merely to avoid reducing test count;
- rewrite historical planning documents for stylistic consistency.

---

# Handoff priority

Execute in this order:

```text
1. Baseline and ownership confirmation
2. abi3-py311 manifest correction
3. release-equivalent wheel tag proof
4. same-wheel CPython 3.11 + 3.14 installed verification
5. service-layer permit-test deletion/replacement
6. live runtime disconnect-recovery proof
7. temporary Plan 115 inventory deletion
8. routine + TLS verification
9. narrow documentation reconciliation
10. Plan 117/118 corrective notes and Plan 119 closure metadata
```

Do not begin documentation closure before the ABI and runtime-admission evidence is real.

The expected implementation is small. If it expands into a broad refactor, stop and reassess against the non-goals above.