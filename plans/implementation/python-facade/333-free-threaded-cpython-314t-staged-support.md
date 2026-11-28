# Python Facade Milestone 333 — Free-threaded CPython 3.14t staged support

> **Amendment — Milestone 330 (2026-11-28).** This record was Milestone 326 on
> its originating branch. It is Milestone 333 here because `main` already
> assigns 326 to the public-bind acknowledgment milestone; every reference in
> the body was renumbered with it. The Beta tier it establishes is unchanged.

Status: implemented (closure closed 2026-11-23: the `python-free-threaded` CI
lane is green and a `publish_target: none` release aggregate contains the
three `cp314t` wheels; see
`plans/closure/python-facade/333-free-threaded-cpython-314t-staged-support.md`)

Approval: repository owner, 2026-11-23. Approved as drafted; no scope
change from the reviewed plan.

Plan number: 333

Repository baseline: `4949a77acfe4114561bd7cfe59a8458ea1983af7`

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md` (Milestone 333)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (Python `http.server`-shaped
  facade; bounded resources)
- `plans/001-terminology-and-domain-model.md` (`Request`, `Response`,
  `ResponseBody`, `Service`)
- `plans/002-long-term-roadmap.md` (phase 4, Python distribution)
- `docs/python-api.md`, `docs/python-http-server-compatibility.md`,
  `docs/toolchain-support.md`

Applicable ADRs:

- none (no accepted ADR governs the wheel ABI family; this plan records the
  decision it executes and does not change architecture ownership)

Primary class: capability (a new runtime family) with invariant obligations
(the GIL family, security policy, confinement, and crate ownership must not
change)

## 1. Objective

Publish `cp314-cp314t` wheels for free-threaded CPython 3.14 on three
release targets (Linux x86_64 glibc, macOS arm64, Windows x86_64), prove the
existing bridge is safe when the GIL is disabled, and record the support
level as **Beta** until the promotion gate in work package H. The GIL-enabled
`cp311-abi3` contract stays exactly as it is.

### Decision being executed (Option B, refined)

- Free-threaded CPython **3.14t only**. 3.13t (experimental, and no longer
  part of the manylinux interpreter inventory) and 3.15t are out of scope.
- `abi3t` (PEP 803) is out of scope. PyO3 0.29.3 already supports the
  feature: its build-config logic selects `abi3t` only for Python >= 3.15
  and otherwise falls back to a version-specific free-threaded build, which
  a probe with `abi3t-py315` on 3.14t confirmed. Adopting it requires a 3.15
  build lane and a second stable tag; that is a separate decision.
- Three targets, not ten: Linux x86_64 glibc, macOS arm64, Windows x86_64.
  ARMv7 QEMU, Windows arm64, and the musl family are excluded because a
  free-threaded interpreter is not currently guaranteed in those lanes.
- **Matrix tier is `required`; support maturity is Beta.** The matrix
  `candidate` tier means "may be built experimentally; never satisfies the
  release aggregate", so a candidate family would not ship at all. Release
  presence and support maturity are therefore recorded separately: the family
  must be in every aggregate, while the public claim is
  `Programming Language :: Python :: Free Threading :: 2 - Beta` until the
  promotion gate is met. This is a refinement of Option B's wording, not a
  widening of scope.

## 2. Why this milestone is ready

- PyO3 0.29.3 is the resolved Python-crate dependency (Plan 325) and supports
  free-threaded builds. Since PyO3 0.28 the `#[pymodule]` default declares
  free-threading support (`Py_MOD_GIL_NOT_USED`); `gil_used = true` is now
  the opt-out.
- maturin 1.14.1 is pinned in `ci.yml` and `release.yml` and is the
  `abi3t`-capable backend (1.14.0+). Free-threaded wheel tags predate that
  pin: the research build used the locally installed 1.9.5 and still emitted
  `cp314-cp314t`, so the tag is not contingent on 1.14.
- `actions/setup-python` supports `python-version: 3.14t`, and
  `uv python install 3.14t` provides a local interpreter. The Plan 267
  blocker "no free-threaded interpreter available" no longer holds.
- The current pypa manylinux/musllinux images list CPython 3.14t/3.15t in
  their Python inventory; the exact interpreter path inside the pinned
  `manylinux_2_17` image must still be verified (work package E).
- Plan 322 (producer admission bounds) is closed, so streamed producers are
  already bounded before this plan adds free-threaded execution.
- No hard dependency is open. `docs/non-goals.md` contains no
  free-threaded or PyPy implementation entry, so this is a support-tier
  change, not a non-goal crossing.

## 3. Current implementation evidence

### 3.1 Correction of the Plan 267 Track A record

`plans/267-free-threaded-long-tail-wheel-feasibility.md` and
`release/plan-267-wheel-feasibility.md` are legacy trace and immutable under
`plans/README.md`. The DEFERRED verdict is not rewritten. Plan 333
supersedes the Track A free-threaded slice only, and the report carries an
additive status-correction banner (requested by the owner) that leaves all
historical text in place.

The stale premises, for the record:

| Plan 267 statement | Current fact |
|---|---|
| "no free-threaded (`-t`) interpreter available in this environment or in routine CI" | `actions/setup-python` serves `3.14t`; `uv python install 3.14t` installed CPython 3.14.5t locally during the research pass |
| "Module declares `#[pymodule]` without `gil_used = false`, so it loads GIL-required by default" | False for PyO3 >= 0.28. The macro emits `Py_MOD_GIL_NOT_USED` when the attribute is absent; the resolved 0.29.3 keeps `sys._is_gil_enabled()` false after import |
| "PyO3 0.29.2 supports ... the `abi3t` tag family" | `abi3t` is PEP 803, Python 3.15+, `abi3t-py315` in PyO3, maturin 1.14.0+. It is not available on 3.13t/3.14t and is not part of this plan |

The retained contract statement ("free-threaded CPython unsupported") also
disagrees with the shipped declaration. Plan 333 removes that mismatch by
qualifying the declaration and documenting the Beta tier. If the plan is not
executed, the fallback is the one-line opt-out described in section 14.

### 3.2 Baseline code and tests

- Module declaration: `crates/eggserve-python/src/lib.rs:1670-1673`
  (`#[pymodule] fn _native`), no `gil_used` attribute.
- Callback path: `PythonCallbackService` invokes the Python handler from
  `spawn_blocking` through `Python::attach`, bounded by
  `max_python_callbacks` (`crates/eggserve-python/src/server/sync_handler.rs:700-725`).
- Streamed producers: one `std::thread` per response, admitted by the Plan
  322 producer semaphore, attaching to Python per item
  (`crates/eggserve-python/src/server/body_bridge.rs:86-215`).
- One-shot state: `RequestBody` (`body_bridge.rs:267-271`), `Response`
  (`response_bridge.rs:22-31`), and the tunnel receiver
  (`tunnel_bridge.rs:243-249`) are `frozen` pyclasses with mutex/take-put
  interiors.
- Iterator: `BodyChunkIterator.__next__` takes `&mut self` and detaches
  around `blocking_recv` (`body_bridge.rs:585-600`).
- Tunnel receive takes the receiver out for one blocking wait; a concurrent
  call reports "tunnel already closed" (`tunnel_bridge.rs:273-300`, NOTE
  B83).
- Server lifecycle: concurrent `start()` is serialized with an atomic swap
  (`server/runtime.rs:482-510`).
- No `unsafe` in the crate (manifest `unsafe_code = "deny"`), no custom
  `Drop`, no mutable module statics; one per-instance `OnceLock` caches the
  request header view (`request_bridge.rs:32`).
- Python-side async bridge uses `run_coroutine_threadsafe`,
  `asyncio.to_thread`, and per-dispatch local state
  (`python/eggserve/lowlevel.py:600-700, 840-900`).

### 3.3 Pre-plan evidence (research only, not qualification evidence)

All runs used an unmodified tree at the baseline commit and a 3.14t
interpreter; only macOS arm64 was exercised.

| Check | Result |
|---|---|
| `cargo check` with a free-threaded `PYO3_CONFIG_FILE` | pass (7.2 s); the bridge compiles under the free-threaded configuration, which is where PyO3 shared-access bounds apply |
| `maturin build -i python3.14t` (unchanged manifest) | `eggserve-0.2.5-cp314-cp314t-macosx_11_0_arm64.whl` |
| install + import under `-X gil=0` | pass; `sys._is_gil_enabled() == False`; no `RuntimeWarning` |
| full Python suite on CPython 3.14.5t | `Ran 853 tests ... OK` (123.2 s) |
| 80 POSTs / 8 workers, buffered bodies + `Response.stream` | 80 callbacks, 0 errors, correct streamed bytes |
| `abi3t-py315` enabled with 3.14t | resolves to `target_abi=CPython-free_threaded-3.14`; compiles |

### 3.4 Known gaps at the baseline

- The module already advertises free-threading support by default, but no
  document says so and no CI lane verifies it; a source build on 3.14t runs
  GIL-free without qualification. This is the primary gap.
- No free-threaded CI lane and no `test_free_threaded.py`; no assertion that
  the GIL stays disabled.
- `release/wheel-matrix.toml` has no ABI-family dimension;
  `scripts/wheel-matrix.py` keys uniqueness on the platform tag alone;
  `scripts/check-release-wheel-set.py` hard-asserts `abi3`/`cp311`;
  `scripts/check-python-release-metadata.py` requires `abi3-py311` and
  rejects per-minor tags; `release.yml` hard-codes
  `--interpreter python3.11`.
- Handler and iterable thread-safety is not part of the documented contract.
- Concurrent `Tunnel.recv` returns a misleading terminal-looking error.

## 4. Invariants that must not regress

1. The GIL-enabled `cp311-abi3` family keeps its tags, wheel count, version,
   and behavior; the existing suite stays green.
2. No new `unsafe`; `unsafe_code = "deny"` stays.
3. Static/path/filesystem confinement, TLS identity, HTTP framing, and crate
   ownership are untouched.
4. `RequestBody` stays one-shot; `Response` stays one-shot; HEAD and
   body-forbidden responses never advance a producer.
5. Bridge Rust state stays behind `Mutex`, atomics, or one-shot take/put; no
   lock is held across a blocking wait except an explicitly reviewed
   serialization gate.
6. No new public Python API or runtime config knob.
7. Support claims never exceed evidence: no 3.13t/3.15t, `abi3t`, PyPy,
   ARMv7, Windows arm64, or musl free-threaded claim.

## 5. Scope

### In scope

- An explicit `#[pymodule(gil_used = false)]` declaration with a comment
  naming the qualified platforms.
- One tunnel-receive contention fix and documentation of the one-shot /
  iterator semantics under free-threading.
- A free-threaded test module and a CI lane.
- A family-aware wheel matrix, validator, and release workflow with three
  `cp314-cp314t` targets.
- Documentation, classifier, and support-tier updates.
- The Plan 267 correction banner and the registry/roadmap updates.

### Explicitly out of scope

- 3.13t and 3.15t support; `abi3t`; PyPy.
- Any platform outside the three named targets.
- Any Rust crate API, HTTP, TLS, or security change.
- TSan/soak as a mandatory CI gate (optional evidence for promotion).
- Performance claims or benchmarks.
- Publication itself: this plan produces the artifacts and evidence; cutting
  a release remains a separate manual action.

## 6. Required production changes

### Crates and ownership

Only `crates/eggserve-python` changes, plus the new test file and the
compile-check script. No other crate is touched; the topology gate must stay
green.

### Config and policy

No `RuntimeConfig` field changes. The release matrix gains an ABI-family
dimension; the support tier changes in metadata and docs only.

### Protocol and compatibility

No wire behavior change. The compatibility statement changes: handlers and
stream iterables may execute Python code in parallel on a free-threaded
interpreter.

### Runtime and concurrency

The declaration becomes explicit; the tunnel receive path gains a contention
guard; one-shot and iterator semantics are documented. No new admission
semantics: `max_python_callbacks` remains the callback and producer bound.

### Frontend or operator surface

Wheel tags, artifacts, and the PyPI classifier. No CLI change.

### Security and confinement

No policy change. Threat-model and extension-contract wording is corrected
where it says "the GIL is released during I/O", which is wrong on a
free-threaded build (the thread detaches from the interpreter; there is no
GIL).

### Documentation and static guards

Family-aware validators and the metadata gate become static guards for the
advertised support set.

## 7. Ordered work packages

### Work package A — Make the declaration explicit and correct the record

Intent: the code's attestation and the support contract must agree.

Required changes:

- `crates/eggserve-python/src/lib.rs`: add
  `#[pymodule(gil_used = false)]` above `fn _native`, with a comment naming
  CPython 3.14t on the three qualified targets. The attribute is a no-op on
  GIL-enabled builds and documents the audited promise.
- `release/plan-267-wheel-feasibility.md`: add the additive
  status-correction banner at the top (no historical line changes).
- Registry and roadmap updates described in work package H.

Acceptance evidence:

- `cargo check` passes for the GIL-enabled and free-threaded configurations.
- Importing the 3.14t wheel keeps `sys._is_gil_enabled()` false.
- `grep -n "pymodule" crates/eggserve-python/src/lib.rs` shows exactly one
  declaration.

### Work package B — Free-threaded concurrency hardening

Intent: remove the one misleading contention path and state the contract
before users meet it.

Required changes:

- `server/tunnel_bridge.rs`: guard `recv` with an in-flight flag
  (compare-exchange; reset on every exit path, including errors). A
  concurrent second `recv` raises a clear `RuntimeError` ("concurrent
  Tunnel.recv is not supported") instead of "tunnel already closed". Do not
  hold a `std::sync::Mutex` across `block_on`.
- `server/body_bridge.rs`: extend the `BodyChunkIterator` doc comment to
  state that concurrent `__next__` raises `Already borrowed` and is not
  supported.
- `Response` and `RequestBody` doc comments: state that concurrent reuse
  raises the existing one-shot errors; no behavior change.
- No new public type or signature.

Acceptance evidence: the tests in work package C pass on 3.14t and the GIL
suite stays green.

### Work package C — Free-threaded test module

Intent: make the free-threading promise testable and regressive.

Required changes: add `crates/eggserve-python/tests/test_free_threaded.py`,
skipped unless `sysconfig.get_config_var("Py_GIL_DISABLED")` is truthy:

1. **Import stays GIL-free**: in a fresh interpreter, import
   `eggserve._native` and assert `sys._is_gil_enabled() is False` and no
   `RuntimeWarning` was raised.
2. **Parallel callbacks with shared state**: N concurrent requests through a
   handler that increments a locked counter; assert the count is exact and
   every response is correct.
3. **Serialized mode**: with `max_python_callbacks=1`, a handler records
   the maximum concurrent entry count and asserts it is 1.
4. **Concurrent stream producers**: concurrent `Response.stream` requests
   deliver the exact expected bytes and respect the producer budget.
5. **One-shot contention**: two threads racing `RequestBody.read()` produce
   exactly one success and one `RequestBodyConsumedError`; two threads
   racing `BodyChunkIterator.__next__` produce a typed error, never a crash.
6. **Tunnel contention**: concurrent `Tunnel.recv` returns the new explicit
   error, not "tunnel already closed".

Use bounded waits and `max_in_flight_requests`/timeouts that keep the suite
fast; no thread-count assertions from Python.

Acceptance evidence: the module skips on the GIL build and passes on 3.14t.

### Work package D — Build configuration and compile check

Intent: make the free-threaded build reproducible and regression-gated.

Required changes:

- Confirm no PyO3 feature change is needed for `cp314-cp314t`: `abi3-py311`
  is ignored on a free-threaded interpreter and PyO3 falls back to a
  version-specific build. Record the observed behavior in the closure.
- `crates/eggserve-python/pyproject.toml`: raise the build-system floor to
  `maturin>=1.14.1,<2.0` to match the pipeline pin and the `abi3t`-ready
  backend. `requires-python` stays `>=3.11`.
- Add `scripts/check-python-free-threaded-compile.sh`:
  - require a `-t` interpreter (default `python3.14t`, `PYTHON=` override);
  - run `PYO3_PYTHON="$PYTHON" cargo check --manifest-path
    crates/eggserve-python/Cargo.toml --locked`;
  - fail with a clear message when the interpreter is absent.
- Record the `PYO3_PRINT_CONFIG=1` output
  (`target_abi=CPython-free_threaded-3.14`, `build_flags=Py_GIL_DISABLED`).

Acceptance evidence: the script passes locally and in CI; the config print is
in the closure.

### Work package E — Family-aware wheel matrix and release gates

Intent: one declarative authority that can express two ABI families without
a second platform list.

Required changes:

- `release/wheel-matrix.toml`: add explicit `abi_family`, `python_tag`,
  `abi_tag`, and `interpreter` keys to every target (migrating the existing
  entries explicitly). Add three `cp314t` entries with
  `abi_family = "cp314t"`, `python_tag = "cp314"`,
  `abi_tag = "cp314t"`, `interpreter = "python3.14t"`, and artifact IDs
  suffixed `-ft`: Linux x86_64 glibc, macOS arm64, Windows x86_64. Keep the
  GIL entries on `python3.11` / `cp311` / `abi3`.
- `scripts/wheel-matrix.py`: validate the new keys; make uniqueness
  `(platform_tag, abi_family)`; emit `abi_family`, `python_tag`,
  `abi_tag`, and `interpreter`; extend `expected-tags`/self-tests.
- `scripts/check-release-wheel-set.py`: require one wheel per
  `(platform, family)`; validate the tag triple per family (allow the
  dotted `abi3.abi3t` form only when a future family declares it); update
  the self-tests to cover a missing family, a wrong family tag, and a
  duplicate family.
- `scripts/check-python-release-metadata.py`: keep `abi3-py311`; accept the
  declared free-threaded family; allow only `cp314t` tags, only for the
  declared targets; keep rejecting `abi3t` claims.
- `release.yml`: use the matrix-emitted `interpreter` field instead of the
  hard-coded `python3.11`; add the family to the artifact names; keep the
  `abi-proof` lane GIL-only; add a free-threaded smoke path (install and
  run `release_smoke.py` plus the GIL assertion). Determine and record the
  interpreter path for each lane: native runners use
  `actions/setup-python` 3.14t; manylinux/musllinux use the image's
  `/opt/python/cp314-cp314t/bin/python` or a python-build-standalone
  install; do not publish a wheel whose interpreter differs from the
  declared ABI.
- Exclude ARMv7, Windows arm64, and the musl family from the free-threaded
  family in this plan.

Acceptance evidence: `wheel-matrix.py validate`, both self-test suites, and
synthetic aggregate checks (complete set passes; missing family fails; wrong
tag fails).

### Work package F — Free-threaded CI lane

Intent: continuous proof instead of a one-off run.

Required changes:

- `ci.yml`: add a `python-free-threaded` job (separate from the existing
  `python` job so failures are unambiguous) with
  `actions/setup-python` `python-version: "3.14t"`,
  `pip install maturin==1.14.1`, the compile-check script, and
  `PYTHON=python3.14t bash scripts/test-python-wheel.sh`.
- Add an explicit assertion after install:
  `python -X gil=0 -W error::RuntimeWarning -c "import sys, eggserve._native; assert sys._is_gil_enabled() is False"`.
- The job runs the full suite, including `test_free_threaded.py`.
- Keep the existing GIL `python` job and its wheel path unchanged.

Acceptance evidence: a green CI run linked in the closure; the job fails if
the module re-enables the GIL or if the new tests fail.

### Work package G — Support tier, documentation, and classifier

Intent: the public claim matches the evidence.

Required changes:

- `docs/toolchain-support.md`: add free-threaded CPython 3.14t (Beta; three
  targets) and state that 3.13t/3.15t/PyPy remain unsupported; update the
  PyO3 version line and the wheel matrix table.
- `docs/python-packaging.md`: list both families, their tags, and the
  platform subsets.
- `docs/release-contract.md` and `docs/release-process.md`: replace the
  single-family statements with the family set; keep free-threaded claims
  Beta; note that the `abi-proof` lane stays GIL-only.
- `docs/library-capability-matrix.md`: split the Python wheel row by
  family.
- `docs/python-api.md` and `docs/python-http-server-compatibility.md`:
  document parallel handler/iterable execution, the caller's thread-safety
  responsibility, `max_python_callbacks=1` as serialized mode, and the
  one-shot contention errors.
- `docs/threat-model.md` and `docs/extension-contract.md`: correct "the GIL
  is released during I/O" to describe interpreter detach on free-threaded
  builds.
- `SECURITY.md`: update the unsupported-implementations sentence.
- `CHANGELOG.md`: additive wheel family and Beta tier.
- `.opencode/skills/eggserve-dev/SKILL.md`: update the wheel-support bullet
  and the Plan 267 pointer.
- `crates/eggserve-python/pyproject.toml`: add
  `Programming Language :: Python :: Free Threading :: 2 - Beta`;
  `scripts/check-python-release-metadata.py` enforces that the classifier
  exists exactly when the family is declared.
- No doc may claim support outside the three targets.

Acceptance evidence: the metadata gate passes; a doc search for
"free-threaded" returns only the Beta statements above.

### Work package H — Closure and promotion gate

Intent: make the tier decision evidence-based and reversible.

Required changes:

- Write `plans/closure/python-facade/333-free-threaded-cpython-314t-staged-support.md`
  using the closure template.
- Promotion gate (Beta to Stable) requires all of:
  - one release in which the family is `required` and the aggregate is
    green on all three targets;
  - the GIL family unchanged across that release;
  - an adversarial/contention evidence pass (TSan on the Rust build, or a
    documented substitute such as a soak with concurrent callbacks,
    streams, and tunnels);
  - no unresolved medium-or-higher finding in the closure.
- Expanding to more platforms, adding `abi3t`, or adding 3.13t/3.15t needs a
  new plan; it is not an extension of this one.

Acceptance evidence: the closure record with the evidence matrix and an
explicit promotion decision (promote or retain Beta with the reason).

## 8. Failure, cancellation, restart, and contention semantics

- **Handler parallelism**: on 3.14t, up to `max_python_callbacks` handlers
  execute Python in parallel. The callback semaphore and pre-response
  admission are unchanged. A handler that shares mutable Python state without
  its own lock may race; that is the caller's responsibility and is
  documented.
- **Stream producers**: bounded by the Plan 322 producer budget; concurrent
  iterables may run in parallel; teardown observation before each pull is
  unchanged.
- **Timeouts**: still close the HTTP request, not Python code; the callback
  permit stays held until the handler returns.
- **One-shot contention**: exactly one caller wins; the others receive the
  existing typed errors; no partial state.
- **Tunnel contention**: the new in-flight guard makes the second concurrent
  `recv` fail with an explicit error; no receiver is lost.
- **Shutdown**: unchanged; no join of arbitrary Python threads; no new
  detached work.
- **GIL re-enabled at runtime**: if the interpreter is started with
  `PYTHON_GIL=1`/ `-X gil=1`, execution serializes and all behavior remains
  correct; the test asserts the module does not re-enable it on its own.

## 9. Compatibility and migration

- Additive: GIL users on 3.11-3.15 keep receiving `cp311-abi3`; 3.14t users
  gain `cp314-cp314t`. No source or API change.
- Existing handlers that are not thread-safe may now be called in parallel on
  3.14t. This is a documented semantic change, not a silent one;
  `max_python_callbacks=1` restores serialization.
- No Rust crate version or semver change. The wheel version stays unchanged
  until a release is cut.
- PyPI publication is effectively irreversible; publish only after the
  closure gates pass. Rollback inside a release window is removing the family
  from the matrix; published wheels cannot be withdrawn reliably.

## 10. Required tests

### Focused unit tests

- Tunnel recv in-flight guard: second concurrent call, guard reset after
  success, error, and EOF.
- Matrix validator self-tests for the family dimension.

### Integration tests

- `test_free_threaded.py` cases 1-6 (work package C).
- Existing suite under 3.14t (the research run is the starting evidence; CI
  becomes the authority).

### Restart and recovery tests

- Server start/stop under 3.14t with concurrent callbacks; shutdown with an
  idle producer.

### Contention and cancellation tests

- Two threads racing `RequestBody.read()`; two threads racing
  `BodyChunkIterator.__next__`; concurrent `Tunnel.recv`; concurrent
  stream producers at budget.

### Security and negative tests

- Import under `-W error::RuntimeWarning` proves no GIL re-enable.
- Concurrent handlers never exceed `max_python_callbacks`.
- No error path logs request/response content.

### Migration and compatibility tests

- The existing GIL suite and wheel-set validator remain green with the family
  added.

## 11. Required verification commands

```bash
# Free-threaded interpreter (local; CI uses actions/setup-python 3.14t)
uv python install 3.14t
PY="$(uv python find 3.14t)"

# Compile under Py_GIL_DISABLED
PYO3_PYTHON="$PY" cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
PYO3_PRINT_CONFIG=1 PYO3_PYTHON="$PY" cargo check \
    --manifest-path crates/eggserve-python/Cargo.toml --locked
# (intentionally halts after printing)
# expect: target_abi=CPython-free_threaded-3.14 and build_flags=Py_GIL_DISABLED

# Wheel, install, and suite
"$PY" -m pip install maturin==1.14.1
PYTHON="$PY" bash scripts/test-python-wheel.sh
"$PY" -X gil=0 -W error::RuntimeWarning -c \
    "import sys, eggserve._native; assert sys._is_gil_enabled() is False"

# Static guards
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-python-release-metadata.py
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
python3 scripts/check-crate-topology.py

# Repository gate
./scripts/verify.sh fast
```

Known baseline hazard: on macOS, `verify.sh fast` currently fails
`application_server_qualification_297::app_bounded_body_accepts_and_over_limit_rejects`
with a `ConnectionReset` (recorded in Plan 325's closure). Record the skip or
failure honestly; do not hide it or change the wrapper.

## 12. Documentation updates

- `docs/toolchain-support.md` (support table, ABI section, PyO3/maturin
  versions, wheel matrix)
- `docs/python-packaging.md` (prerequisites, both families, tags)
- `docs/release-contract.md`, `docs/release-process.md` (family set,
  Beta tier, `abi-proof` scope)
- `docs/library-capability-matrix.md` (Python wheel row)
- `docs/python-api.md`, `docs/python-http-server-compatibility.md`
  (parallel execution and contention contract)
- `docs/threat-model.md`, `docs/extension-contract.md` (GIL wording)
- `SECURITY.md`, `CHANGELOG.md`
- `.opencode/skills/eggserve-dev/SKILL.md`
- `plans/subsystems/python-facade-roadmap.md`, `plans/registry.md`,
  `release/plan-267-wheel-feasibility.md` (correction banner)

## 13. Acceptance criteria

1. `cp314-cp314t` wheels build for Linux x86_64 glibc, macOS arm64, and
   Windows x86_64, with correct tags, and install on CPython 3.14t.
2. Importing the extension on 3.14t leaves the GIL disabled and raises no
   `RuntimeWarning`.
3. The full Python suite plus `test_free_threaded.py` passes on 3.14t.
4. The GIL `cp311-abi3` family keeps its tags, wheel count, and suite
   behavior.
5. The wheel matrix, release-set validator, metadata gate, and release
   workflow understand exactly two families on the declared platform subsets.
6. The tunnel contention fix returns the explicit error; no one-shot state is
   lost.
7. Documentation and the PyPI classifier state Beta for 3.14t on the three
   targets only.
8. No security, confinement, TLS, HTTP-tier, or Rust public-API change.

## 14. Stop conditions

Stop and report rather than improvise when:

- any free-threaded test shows a crash, data corruption, or a bridge state
  race; the fallback is `#[pymodule(gil_used = true)]` plus the documented
  unsupported contract, not a partial GO;
- the family-aware matrix would require changing the meaning of the existing
  `tier` values or keeping a second platform list;
- a required target has no free-threaded interpreter and no reproducible
  install path; narrow the family rather than publish an ABI-mismatched
  wheel;
- `abi3t`, 3.15, or 3.13t becomes necessary to satisfy the tag set (out of
  scope; new plan);
- the GIL family's tags or suite regress;
- the fix requires a new public Python API or runtime config knob.

## 15. Closure evidence required

- Exact wheel filenames and tags for all three targets, with the platform
  matrix revision.
- CI run URLs for the free-threaded job and the existing lanes.
- `sys._is_gil_enabled()` output from the installed wheel, with the
  `RuntimeWarning` assertion enabled.
- Test counts and output for the full suite and `test_free_threaded.py`.
- The 80-request/8-worker stress output (or the CI equivalent).
- Matrix/validator self-test output, including the negative cases.
- `PYO3_PRINT_CONFIG` output showing the free-threaded target ABI.
- Proof that the GIL `cp311-abi3` family is unchanged.
- Documentation and classifier diffs.
- Residual risks and the promotion decision (promote or retain Beta).

## 16. Handoff notes

- **Excluded-crate Cargo gotcha**: `crates/eggserve-python` is excluded from
  the workspace and has its own lockfile. Running Cargo from a checkout nested
  inside another workspace root can fail with "current package believes it's
  in a workspace when it's not". Run Python-crate Cargo commands from the
  primary checkout or a standalone worktree.
- The free-threaded interpreter is not installed by default. Install it with
  `uv python install 3.14t` or `actions/setup-python` `3.14t`; record the
  exact build and version in the closure.
- The pre-plan evidence is macOS arm64 only and is not qualification evidence.
  Linux and Windows proof must come from CI or a qualifier.
- Do not add `abi3t-py315` opportunistically. The probe shows it is safe, but
  it changes the 3.15 tag story and needs its own plan.
- Keep the family out of ARMv7/Windows arm64/musl until a free-threaded
  interpreter is proven there.
- The macOS baseline test failure in section 11 is pre-existing; do not
  "fix" it by weakening the wrapper or the suite.
- `verify.sh full` needs Python 3.14 plus maturin; the free-threaded lane is
  separate and must not depend on the local 3.14 GIL interpreter.
