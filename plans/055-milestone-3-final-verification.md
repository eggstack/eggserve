# Phase 55 — Milestone 3 Final Verification

## Goal

Verify that the completed Milestone 3 implementation is operationally and evidentially closed on one exact commit. This phase is not an implementation expansion. It proves that the shared Rust runtime, Python delegation, lifecycle semantics, TLS service parity, callback timeout behavior, cross-platform wheel behavior, and release evidence all agree with the documented contract.

This phase must not add routing, middleware, request-body support, ASGI/WSGI adapters, HTTP/2, reverse proxy behavior, authentication, or new public server features.

## Starting state

The repository now has:

- one Rust-owned server implementation used by CLI, Rust embedding, and Python;
- `Server`, `ServerBuilder`, `ServerHandle`, `Service`, and `RuntimeConfig` APIs;
- lifecycle states `Created`, `Starting`, `Running`, `Draining`, `Stopped`, and `Failed`;
- graceful and forced shutdown with task tracking;
- Python callback dispatch through `PythonCallbackService`;
- bounded callback concurrency and documented non-interruptible callback semantics;
- shared plaintext/TLS service dispatch;
- installed-wheel lifecycle tests;
- dedicated runtime and lifecycle release criteria;
- fail-closed release evidence aggregation.

Remaining verification obligations:

1. prove every required runtime gate emits a distinct evidence record;
2. inspect Linux, macOS, and Windows installed-wheel evidence from the same SHA;
3. verify that runtime/TLS/Python gates cannot be accidentally satisfied by broad aggregate test jobs;
4. validate final-SHA freshness, artifact digests, and generated checklist reproducibility;
5. confirm blocked Python callbacks cannot prevent runtime shutdown or create unbounded worker growth;
6. ensure only read-only lifecycle state is public where possible;
7. record the exact CI run and artifacts used to declare Milestone 3 closed.

## Track A — Freeze the verification candidate

Select one clean main-branch commit after all Phase 54 implementation and test fixes.

Requirements:

- working tree clean;
- commit reachable from `main`;
- no unreviewed local patches;
- criteria, generated checklist, docs, and API snapshots clean;
- exact Rust toolchain, Python version, platform matrix, and dependency lockfiles recorded;
- no evidence from earlier SHAs reused.

Create a verification manifest containing:

- commit SHA;
- commit timestamp;
- criteria schema version;
- release criteria digest;
- Cargo.lock digest;
- Python package metadata/version;
- workflow run IDs;
- expected artifact names;
- verification operator/date.

Acceptance:

- one exact commit is the sole Milestone 3 verification candidate;
- all evidence records and artifacts reference that SHA.

## Track B — Verify dedicated evidence emission

Inspect CI commands and produced JSON records for every required Milestone 3 gate.

Required gate identities:

- `runtime.public-rust-consumer`;
- `runtime.service-dispatch`;
- `runtime.listener-lifecycle`;
- `runtime.graceful-shutdown`;
- `runtime.forced-shutdown`;
- `runtime.tls-service-parity`;
- `python.runtime-parity`;
- `python.callback-timeout`;
- `python.lifecycle-linux`;
- `python.lifecycle-macos`;
- `python.lifecycle-windows`;
- any existing `python.resource-qualification` or contract-consistency gate retained by the criteria model.

For each gate verify:

- one stable gate ID;
- one explicit command or test selection;
- correct workflow/job identity;
- correct trigger policy;
- platform/target metadata;
- exact commit SHA;
- start/end timestamps and exit code;
- artifact references when required;
- invalidation paths;
- freshness rules;
- no gate is satisfied solely because another broad job passed.

Add contract-consistency or release-safety tests that fail if:

- a required gate has no wrapper invocation;
- two gates write the same evidence filename;
- a platform gate is mapped to the wrong runner;
- a gate command no longer selects the intended tests;
- a required gate is marked passed without evidence.

Acceptance:

- all Milestone 3 gates have distinct, inspectable evidence records;
- criteria-to-workflow drift fails CI.

## Track C — Execute the complete main-branch matrix

Run the full main-branch workflow for the verification candidate.

Required execution:

- Rust workspace format, lint, unit, integration, doctest, feature, and package checks;
- runtime public-consumer tests;
- service dispatch tests;
- listener and lifecycle integration tests;
- graceful and forced shutdown tests;
- TLS custom/static service parity tests;
- Python source-tree tests;
- Python parity and resource qualification suites;
- installed-wheel tests on Linux, macOS, and Windows;
- package/wheel smoke tests;
- supply-chain checks;
- evidence aggregation and checklist generation.

Do not substitute local success for missing CI platform evidence.

Acceptance:

- all required jobs complete on the same SHA;
- skipped jobs are explicitly `NOT_APPLICABLE` only where criteria permit;
- no required platform gate is absent.

## Track D — Inspect cross-platform installed-wheel behavior

Download the Linux, macOS, and Windows wheel artifacts and their evidence.

For each platform verify:

- wheel filename and compatibility tags;
- wheel SHA-256 digest;
- installed package version;
- native extension loaded from the installed wheel;
- source-tree imports disabled;
- bundled binary resolution does not fall back to the repository;
- port-zero startup;
- readiness;
- all lifecycle states observable where timing permits;
- static response;
- callback response and exception containment;
- callback timeout returns documented response;
- callback capacity remains bounded;
- graceful shutdown;
- forced shutdown;
- repeated new-instance start/stop cycles;
- address reuse according to platform policy.

Platform notes:

- Windows must not be judged by Unix signal semantics;
- timing assertions should be bounded but tolerant of shared runners;
- macOS and Windows must run native wheel code, not source builds;
- any exclusion must be narrow, documented, and represented in criteria.

Acceptance:

- all advertised wheel platforms have valid lifecycle evidence;
- no platform-specific behavior contradicts the support matrix.

## Track E — Verify blocking callback containment

Exercise callbacks that:

- complete normally;
- exceed the handler deadline and later return;
- raise exceptions;
- return invalid response values;
- return coroutine objects;
- block on a controllable event across graceful shutdown;
- remain blocked across forced shutdown;
- saturate all callback permits;
- repeatedly time out.

Verify:

- request tasks stop waiting at the configured deadline;
- timeout response is deterministic;
- callback permit remains accounted for until Python returns;
- no additional callback executes beyond `max_python_callbacks`;
- socket and connection permits are released after request timeout;
- graceful shutdown waits only within its deadline;
- forced shutdown terminates the Rust runtime and closes the listener;
- blocked Python work does not retain the listener or runtime task registry;
- repeated timeout attempts do not create unbounded threads/tasks;
- process-exit limitations are documented accurately.

Use counters or test-only observability where needed. Do not add a broad production metrics API in this verification phase.

Acceptance:

- callback behavior matches the documented non-cancellation contract;
- resource usage remains bounded under repeated timeout and saturation tests.

## Track F — Audit lifecycle API exposure

Review public exports in `eggserve_core::server`.

Preferred public surface:

- `LifecycleState` is public and read-only;
- `ServerHandle::state()` exposes state;
- transition/control internals remain private or crate-private;
- external callers cannot force arbitrary invalid lifecycle transitions;
- Python reads state through the real handle.

If the full `Lifecycle` object is currently public only for binding access, narrow it or document why it must remain experimental.

Tests:

- external consumer compile fixture can read state but cannot invoke internal transitions;
- API stability inventory matches actual exports;
- no private lifecycle internals leak through Python types/stubs.

Acceptance:

- lifecycle control remains encapsulated;
- public experimental surface is no broader than required.

## Track G — Reproduce evidence aggregation locally

Download all evidence artifacts from the selected CI run into a clean directory.

Run:

```sh
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py aggregate --criteria release/criteria.toml --evidence <downloaded-evidence>
python3 scripts/release_criteria.py generate-checklist --criteria release/criteria.toml --evidence <downloaded-evidence>
```

Verify:

- aggregate exit status is successful;
- every required Milestone 3 gate is `PASSED` or an explicitly permitted state;
- no stale, invalidated, malformed, conflicting, or missing record exists;
- exact-SHA matching succeeds;
- artifact digests resolve;
- regenerated checklist is byte-for-byte deterministic;
- file ordering does not alter output;
- removing one required runtime evidence file makes aggregation fail;
- replacing one SHA marks the record stale/invalid;
- modifying one artifact digest fails validation.

Acceptance:

- downloaded evidence independently reproduces the CI checklist;
- fail-closed mutation checks behave as designed.

## Track H — Documentation and capability reconciliation

Verify that documentation matches the final behavior:

- Python uses the actual Rust runtime;
- callback timeout does not cancel arbitrary Python execution;
- concurrency accounting remains active after timeout until callback completion;
- forced shutdown closes runtime/listener but cannot kill Python code safely;
- TLS and plaintext share the same service path;
- pre-bound listener support is Rust-only unless implemented later;
- server APIs remain experimental;
- all six lifecycle states are documented consistently;
- supported wheel platforms and evidence requirements match criteria.

Run contract consistency checks and add targeted assertions for these claims.

Acceptance:

- no stale “same pattern” or “best effort transport timeout” language remains;
- capability matrix and release contract match implementation and criteria.

## Track I — Record verification outcome

Create a concise Milestone 3 verification record under an appropriate release/evidence location or generated documentation path.

Record:

- verified commit SHA;
- CI run ID/URL;
- artifact IDs/names;
- aggregate manifest digest;
- generated checklist digest;
- platform results;
- any allowed exclusions;
- known limitations;
- verifier and date.

Do not claim public release readiness unless all unrelated release gates are also satisfied. This record closes Milestone 3 only.

## Required validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo test -p eggserve-core --test server_integration
cargo test -p eggserve-core --test lifecycle_integration
cargo test -p eggserve-core --features tls --test tls_service_parity
cargo test -p eggserve-core --test public_api_consumers
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_check_contract_consistency -v
python3 -m unittest scripts.test_release_safety -v
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py generate-checklist --check
bash scripts/release-validate.sh full
```

Run all installed-wheel packaging tests on Linux, macOS, and Windows through CI.

## Completion criteria

Milestone 3 verification is complete only when:

- one exact main-branch SHA has complete runtime evidence;
- every dedicated runtime/Python/platform gate emits a distinct valid record;
- Linux, macOS, and Windows wheel lifecycle evidence is present;
- TLS custom/static service parity passes;
- blocking callback timeout and forced-shutdown behavior are bounded and truthful;
- lifecycle internals are appropriately encapsulated;
- downloaded evidence regenerates the same checklist;
- mutation tests prove aggregation remains fail-closed;
- documentation and capability claims match the verified implementation;
- a verification record identifies the run and artifacts.

## Non-goals

- No request-body support.
- No runtime API promotion to stable.
- No new listener capabilities.
- No async Python callback support.
- No publication or tag creation.
- No general observability framework.