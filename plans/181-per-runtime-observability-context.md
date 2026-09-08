# Plan 181 — Per-Runtime Observability Context

## Status

**PLANNED — embedding-quality improvement; no telemetry-framework expansion.**

Prerequisites: Plan 178 closed; Plan 180 preferably closed so observability is plumbed through clearly separated runtime/driver modules rather than deepening the current monolithic connection module. Plan 179 should also be closed so `RuntimeState` construction rests on validated configuration.

## Purpose

Replace EggServe's runtime dependence on process-global logging/counters with an explicit per-runtime observability context while preserving the current simple CLI/default behavior.

The current `eggserve_core::ops` model is useful and deliberately lightweight: structured `Event`, pluggable `LogSink`, counters, snapshots, and connection correlation IDs. The limitation is ownership. `Logger` and `OpsCounters` are currently reached through process-global `OnceLock` values, making independent embedding awkward when one process owns multiple EggServe servers or caller-owned runtimes.

The target is a small `OpsContext`-style object carried by runtime state. It should provide isolation and inspectability without adding `tracing`, OpenTelemetry, metrics exporters, async logging queues, or another broad dependency.

## Current-state findings

- `ops.rs` exposes a public event/sink/counter model.
- the runtime commonly reaches it through `Logger::global()` and `global_counters()`;
- connection IDs also have global/static generation paths;
- the capability matrix classifies observability as minimal;
- the crate-level API-status prose exposes `ops` publicly but does not classify its stability tier as clearly as `primitives` and `server`;
- the CLI initializes a stderr sink and should retain a zero-ceremony path.

Process-global observability causes several library-quality problems:

1. independently embedded servers cannot naturally route events to different sinks;
2. per-server counters cannot be read without contamination from unrelated runtime instances;
3. tests that install global logging state are harder to isolate;
4. caller-owned connection runtimes do not have an explicit telemetry identity despite already sharing a `RuntimeState` object.

## Design constraints

- Preserve the existing lightweight event vocabulary and counter semantics unless a correctness issue requires a narrow correction.
- Preserve a convenient default/global path for the CLI and backwards compatibility.
- Runtime code should use an explicit context once one exists rather than silently falling back to globals at arbitrary call sites.
- Do not add `tracing`, OpenTelemetry, Prometheus, exporter protocols, background log workers, or unbounded channels.
- Do not make observability required for consumers; a no-op/default context remains valid.
- Keep canonical request/response types free of logger/metrics implementation types.
- Keep per-request observability bounded and allocation-conscious.

## Track A — Inventory every global observability use

Before changing APIs, search the entire repository for:

- `Logger::global()`;
- `global_counters()`;
- global correlation ID generators/statics;
- direct stderr/log output from library crates;
- tests depending on global initialization order.

Classify each use as:

1. runtime/server-owned and therefore eligible for explicit context plumbing;
2. standalone primitive/helper code that genuinely lacks runtime ownership;
3. frontend/CLI initialization;
4. test-only behavior.

The goal is not blindly eliminating every global function. The goal is ensuring live server/connection execution has an explicit owner.

## Track B — Introduce a lightweight context

### B1. Context contents

Create a small cloneable context, likely `Arc`-backed, containing at least:

- the active `LogSink`/logger implementation;
- `OpsCounters`;
- connection correlation-ID source.

Prefer composition of existing types over rewriting the event model.

The context should support:

- no-op/default construction;
- explicit sink construction for embedders;
- counter snapshot retrieval;
- cheap cloning into connection tasks.

### B2. Failure semantics

Carry forward Plan 178's non-recursive log-sink panic containment. A per-runtime context must not reintroduce recursion or cross-context fallback through `Logger::global()`.

If a sink fails, failure accounting belongs to the same context whose sink failed.

### B3. Stability classification

Decide and document the public stability tier deliberately. A reasonable default is:

- event/sink/counter primitives remain semver-considered pre-1.0 if already relied upon publicly;
- runtime attachment methods remain experimental with `server`.

Do not implicitly promote the entire runtime API to stable merely because observability becomes explicit.

## Track C — Attach observability to runtime ownership

### C1. `RuntimeState`

Make `RuntimeState` own or reference the observability context. Preserve a simple constructor using a default context and add an explicit construction/builder path for embedders.

Avoid requiring every caller to pass both `RuntimeState` and an independent context to `serve_http1_connection`; the existing shared runtime-state object is the natural ownership point.

### C2. TCP/TLS `Server`

Allow `ServerBuilder` or the equivalent experimental configuration path to select an observability context/sink. The CLI/Python server should continue getting their expected default stderr/no-op behavior without new mandatory configuration.

### C3. Caller-owned connection driver

Use the context from the supplied `RuntimeState` for:

- connection correlation IDs;
- parser/body/admission/timeout/lifecycle events;
- counters;
- shutdown/connection outcome observability.

Remove the separate static connection-ID source in the caller-owned path if the context can become the single correlation authority without changing documented explicit-ID behavior.

`serve_http1_connection_with_id()` must continue honoring a caller-supplied ID where that public/experimental contract exists.

## Track D — Plumb context through internal runtime modules

After Plan 180, each connection responsibility should receive only the observability capability it needs, preferably via a shared context reference rather than independent logger/counter arguments.

Audit the following classes carefully:

- accept/reject and active-connection accounting;
- service admission;
- parser/header/target rejection;
- body policy and deferred-body lifecycle;
- handler panic/timeout;
- keep-alive/write/total timeouts;
- graceful/forced shutdown;
- file/response streaming counters;
- request lifecycle cancellation;
- sink failure accounting itself.

Every existing counter/event used by normal server operation should either move to the explicit context or be intentionally documented as process-global. Accidental mixed ownership is not acceptable.

## Track E — Provide runtime-local inspection

Expose a narrow way for embedders to retrieve an `OpsSnapshot` from the runtime context.

Prefer access through existing experimental ownership objects such as `RuntimeState` and/or `ServerHandle` rather than introducing a monitoring server or endpoint.

Requirements:

- snapshot reads are non-blocking/bounded;
- no reset-on-read semantics unless already supported;
- no exporter/HTTP endpoint is added;
- process-global compatibility helpers may continue to expose their own default context snapshot where useful.

## Track F — Isolation and compatibility tests

### F1. Two-runtime isolation

Construct two runtime contexts with recording sinks/counters and prove:

- events from runtime A do not appear in runtime B's sink;
- connection/request counters remain independent;
- correlation-ID generation is internally coherent per context;
- a failing sink in one context does not affect the other.

### F2. Default compatibility

Prove existing no-explicit-context construction continues to work for:

- Rust `Server`;
- CLI initialization;
- Python native server path;
- caller-owned driver using `RuntimeState::new` or its compatibility successor.

### F3. Concurrent operation

Run two servers/runtimes concurrently in the same process to ensure context ownership is not merely a sequential-test artifact.

## Track G — Capability/documentation reconciliation

Use this implementation pass to correct the small current-state documentation inconsistencies discovered in the review, because they directly concern public/runtime capability inventory.

### G1. Classify `ops`

Update crate/API stability documentation so `pub mod ops` has an explicit stability classification consistent with the rest of the crate.

### G2. Correct runtime TLS capability inventory

Audit `docs/library-capability-matrix.md` against actual feature-gated runtime TLS support and correct rows that currently imply the experimental Rust runtime lacks TLS where the runtime/config/server path actually implements it.

Do not broaden TLS features; this is documentation truthfulness only.

### G3. Remove stale client-policy text after source confirmation

`docs/non-goals.md` currently refers to an existing experimental HTTP client/client feature. Before editing, confirm the current manifest/source tree still has no retained client feature/module. If confirmed, remove or rewrite the obsolete language so non-goals describe the repository that exists.

Do not use documentation cleanup to reintroduce a client.

### G4. Update observability capability matrix

After per-runtime context/snapshot support lands, replace vague `minimal` wording with precise support claims. Do not claim tracing/exporter integrations that are not provided.

## Verification

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features tls
cargo test -p eggserve-bin --features tls
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
python3 scripts/verify-conformance-matrix.py
```

Add targeted concurrent two-runtime observability tests. Do not add a new external observability service or CI lane.

## Acceptance criteria

- [ ] normal server/connection execution can use an explicit per-runtime observability context.
- [ ] runtime A and runtime B can use independent sinks and counters in one process without cross-contamination.
- [ ] connection correlation IDs are owned coherently by the runtime context, with explicit caller IDs still honored.
- [ ] existing CLI/default construction remains simple and compatible.
- [ ] caller-owned `serve_http1_connection` obtains observability through shared runtime ownership rather than unrelated globals.
- [ ] Plan 178 sink-panic containment remains non-recursive and context-local.
- [ ] embedders can retrieve a bounded runtime-local counter snapshot without a monitoring endpoint.
- [ ] canonical request/response primitives remain free of observability implementation types.
- [ ] no tracing/OpenTelemetry/Prometheus/exporter dependency or unbounded logging queue is added.
- [ ] `ops` has an explicit API stability classification.
- [ ] runtime TLS capability documentation matches implementation.
- [ ] stale client-feature documentation is removed if the implementation audit confirms the client no longer exists.
- [ ] all Rust, TLS, Python, Plan 175 consumer, and conformance tests remain green.

## Suggested implementation order

1. Inventory/classify every global logger/counter/correlation use.
2. Introduce `OpsContext` (or equivalent) around the existing event/counter types with Plan 178 failure semantics.
3. Attach it to `RuntimeState` with a compatibility default constructor.
4. Plumb TCP/TLS server and caller-owned driver execution through the runtime context.
5. Migrate connection/request/deferred-body/streaming event and counter sites.
6. Add runtime-local snapshot access and two-runtime isolation tests.
7. Reconcile API/capability documentation in Track G.
8. Run full verification and record any intentionally retained global-only helpers in the closure record.

## Handoff

After closure, EggServe's runtime substrate should have explicit ownership for configuration, connection state, and observability. Future exporter integrations should live downstream or behind a separately justified adapter; do not grow the core into an observability framework.