# Plan 249 — Core auto-H1 authority and shutdown-forwarder lifetime corrective

## Purpose

Correct two residual defects discovered by post-Plan-248 source review at
`4b2af07991d20234d5167d08311ba6b18006025a`.

Plans 243–248 materially improved the repository, but Plan 244's single-H1-
authority closure is not complete:

1. direct H1 entry points in `eggserve-core::server::connection` delegate to
   `eggserve-server`, but the compatibility server's ordinary cleartext
   accept path still enters `WireProtocol::Auto`; after cleartext protocol
   classification, core can resolve that path to `WireProtocol::Http1` and
   execute its own `hyper_builder(...).serve_connection(...)` /
   `drive_connection(...)` pipeline;
2. each accepted compatibility connection spawns a detached shutdown-forwarder
   task that waits on a broadcast receiver until whole-server shutdown. When
   the connection completes normally, that detached task remains alive, so a
   long-running process can accumulate roughly one sleeping Tokio task per
   historical connection rather than per active connection.

This plan fixes both defects without changing any public Rust/Python API,
feature name, protocol support tier, request/response behavior, timeout,
framing, proxy/TLS semantics, or listener capability.

Plan 250 is the mandatory closure/evidence pass.

## Baseline

Planning baseline:

```text
4b2af07991d20234d5167d08311ba6b18006025a
docs: close plans 242-248 with CI evidence
```

The baseline is green in the normal Rust/Python/supply-chain CI matrix. This
corrective therefore treats behavior compatibility as a hard constraint.

## Confirmed residual H1 path

At the baseline, the compatibility accept path dispatches cleartext TCP,
PROXY-prefixed cleartext TCP, and Unix-domain connections through:

```text
accept.rs
  -> serve_http_connection_with_id_and_protocol(..., WireProtocol::Auto)
     -> serve_selected_with_token(...)
        -> classify_cleartext(...)
           -> WireProtocol::Http1
              -> core hyper_builder(...)
              -> core Hyper H1 connection
              -> core drive_connection(...)
```

The explicit `WireProtocol::Http1` branch in
`serve_http_connection_with_id_and_protocol` already delegates to
`eggserve_server::connection::serve_http1_connection_with_id`; the defect is
that `Auto` classification happens after core has committed to the core
Hyper-service/driver machinery.

The corrective must classify protocol before constructing a core H1 pipeline.

## Target architecture

```text
compatibility listener / caller-owned multiprotocol stream
        |
        | core-owned pre-HTTP composition:
        | PROXY preamble, TLS handshake/ALPN, H2 prior-knowledge sniff,
        | Unix/systemd listener metadata
        v
protocol selected
   |                         |
   | H1                      | H2
   v                         v
eggserve-server          eggserve-core
single H1 authority      H2-specific transport/composition
   |
canonical Service / RuntimeState projection
```

Core may continue to own H2-specific Hyper execution and the bounded
cleartext H2-prior-knowledge classifier. Core must not construct or drive a
Hyper H1 connection after this plan.

## Track A — move Auto classification before H1 execution

Refactor the compatibility multiprotocol path so `WireProtocol::Auto` is
resolved while the code still owns:

- the raw/replayable byte stream;
- the canonical `Service`;
- the compatibility `RuntimeConfig`;
- the compatibility `RuntimeState`;
- `ConnectionContext`;
- `ConnectionShutdown`;
- the stable connection ID.

The classifier must return enough information to preserve all bytes consumed
during sniffing. Existing `PrefixedIo`/replay behavior may be reused.

Preferred flow:

1. if the requested protocol is explicit `Http1`, delegate immediately to
   `eggserve_server::connection::serve_http1_connection_with_id`;
2. if explicit `Http2`, enter the existing H2-specific compatibility path;
3. if `Auto`, perform the existing bounded cleartext H2 preface
   classification before creating a Hyper service;
4. if classification resolves H1, delegate the replayable stream to the direct
   H1 driver;
5. if classification resolves H2, construct the compatibility H2 Hyper
   service and enter only H2-specific execution.

Do not classify by peeking unbounded input and do not weaken the existing
header/preface timeout behavior.

When `http2` is not enabled, preserve current H1 behavior without adding an
H2 dependency or silently accepting an H2 runtime capability.

## Track B — remove executable core H1 driver authority

After Track A is working, remove or make unreachable the core implementation
pieces that exist only to drive H1.

At minimum inspect and eliminate production use of:

- core `hyper_builder` for HTTP/1;
- `hyper::server::conn::http1::*` connection construction;
- the `WireProtocol::Http1` branch inside the core resolved Hyper driver;
- core `serve_connection` / `serve_hyper_with_token` helpers if their only
  remaining purpose is H1;
- any compatibility helper that still creates a core Hyper H1 service before
  delegation.

Do not delete a shared helper merely because its module name says H1 if H2
still legitimately consumes protocol-neutral lifecycle/request/response code.
Classify remaining connection modules as:

- H2-specific execution;
- protocol-selection/replay composition;
- protocol-neutral compatibility adapter;
- direct H1 facade/projection.

The source should make those categories obvious.

### Public `serve_connection_with_runtime_state`

If the historical public
`eggserve_core::server::connection::serve_connection_with_runtime_state`
entry point remains part of the accepted surface, keep its exact signature and
semantics.

It must delegate to the direct H1 authority rather than use a private core H1
driver. Adapt its broadcast shutdown receiver to a
`ConnectionShutdown` within the same task/future. Do not create a detached
forwarder solely to preserve this compatibility path.

If source inspection proves this item is not public/accepted, do not remove it
under assumption; record the API evidence first.

## Track C — eliminate detached per-connection shutdown forwarders

The baseline compatibility accept path contains the equivalent of:

```rust
let forwarder_rx = shutdown_rx.resubscribe();
tasks.spawn(async move {
    let conn_shutdown = ConnectionShutdown::new();
    let forwarder_shutdown = conn_shutdown.clone();
    tokio::spawn(async move {
        let _ = forwarder_rx.recv().await;
        forwarder_shutdown.shutdown();
    });

    // serve connection
});
```

The inner `tokio::spawn` is not owned by the connection `JoinSet`. Normal
connection completion drops the outer task but leaves the forwarder parked on
the broadcast receiver until whole-server shutdown.

Replace this with structured lifetime ownership.

Preferred shape inside the already-owned connection task:

```text
pin connection future
pin server-shutdown receive future

select:
  connection completes ->
      drop shutdown receiver/future immediately
      return
  server shutdown arrives ->
      signal ConnectionShutdown
      await/drain the connection according to existing semantics
      return
```

Equivalent helper abstractions are acceptable if their lifetime is still
strictly subordinate to the connection task.

Required properties:

- zero detached per-connection forwarder task;
- receiver/future is dropped when a connection completes normally;
- shutdown arriving before the serve future begins polling is not lost;
- shutdown arriving during PROXY/TLS/protocol classification still reaches
  the connection path according to existing lifecycle semantics;
- no additional unbounded queue or task registry;
- connection admission permits and active-connection gauges remain balanced.

Do not replace the broadcast with a new public shutdown API in this plan.

## Track D — deterministic regression coverage

### Auto → H1 authority test

Add a test that drives an ordinary cleartext compatibility-server connection
through the normal accept path and proves it reaches the direct H1 authority.

Do not rely only on source markers. Use an implementation-observable test hook
that is test-only or an existing direct-runtime observable where practical.

At minimum cover:

- plain TCP cleartext H1;
- cleartext H1 after enabled PROXY preamble/replay;
- Unix-domain H1 on Unix;
- TLS ALPN H1 if the TLS suite has a suitable deterministic fixture.

The test must distinguish "explicit public direct entry point works" from
"normal compatibility accept path delegates".

### Auto → H2 preservation

With `http2` enabled, retain prior-knowledge H2 classification and H2
execution behavior. Include a regression proving the Auto classifier still
selects H2 and does not hand the H2 preface to the direct H1 parser.

### Shutdown-forwarder lifetime

Refactor shutdown forwarding behind a small internal helper if needed so its
receiver lifetime is testable without counting all Tokio runtime tasks.

A deterministic regression should prove that after a normally completed
connection:

- the connection-owned shutdown receiver/future has been dropped;
- no connection-specific forwarder remains waiting for whole-server shutdown.

One acceptable approach is a unit test using
`broadcast::Sender::receiver_count()` around the helper/connection dispatch
lifetime. Test-only counters/guards are also acceptable.

Also cover server shutdown while the connection is active to prove the
structured forwarder still signals `ConnectionShutdown`.

### Repetition

Exercise many short sequential connections and verify:

- active connection gauge returns to zero;
- connection permit capacity returns to baseline;
- shutdown-forwarder receiver count does not grow with historical connection
  count;
- final shutdown/wait completes.

Avoid sleep-only tests.

## Track E — strengthen topology/authority gates

The current Plan 244 gate is insufficient because it only checks that a direct
delegation call exists somewhere in core.

Strengthen `scripts/check-crate-topology.py` so core cannot regain executable
H1 authority.

The gate should reject, in production core connection code:

- a `fn hyper_builder` that constructs an HTTP/1 builder;
- `hyper::server::conn::http1::Connection` /
  `UpgradeableConnection` execution ownership;
- a resolved `WireProtocol::Http1` branch that drives Hyper directly;
- a second `serve_http1_connection` implementation rather than a facade;
- detached per-connection shutdown forwarder spawning in `accept.rs`.

Prefer semantic markers/ownership assertions over line-count thresholds.

The gate must continue to allow:

- H2 Hyper builder/connection ownership in core;
- the bounded H2 prior-knowledge classifier;
- `PrefixedIo` or replacement replay composition;
- type names/re-exports needed for source compatibility;
- direct calls into `eggserve_server::connection::*`.

Add a short comment documenting why H1 markers are forbidden in core.

## Track F — API/capability preservation

No existing public Rust path, Python path, feature flag, or transport
capability may change.

Specifically preserve:

- `eggserve_core::server::connection::{serve_http1_connection,
  serve_http1_connection_with_id}`;
- multiprotocol `serve_http_connection*` when `http2` is enabled;
- `serve_connection_with_runtime_state` if currently public;
- TCP, prebound TCP, Unix, systemd activation, PROXY protocol, TLS, H2 and H3
  composition;
- caller-owned transport semantics;
- request/response framing and timeout behavior;
- connection IDs/observability fields;
- tunnel behavior;
- all current defaults.

No support tier changes.

## Focused qualification

Run at minimum:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo clippy -p eggserve-server --all-targets -- -D warnings
cargo test -p eggserve-server
cargo clippy -p eggserve-core --lib --tests -- -D warnings
cargo test -p eggserve-core
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo test -p eggserve-core --test direct_h1_parity
cargo test -p eggserve-core --test direct_service_convergence
```

Also run targeted PROXY, Unix, caller-owned transport, tunnel, shutdown/drain,
and H2 prior-knowledge tests affected by the dispatch change.

Plan 250 owns full package/wheel/supply-chain/platform/remote-CI closure.

## Acceptance criteria

- [ ] ordinary compatibility cleartext H1 reaches
      `eggserve-server` H1 execution after Auto classification;
- [ ] PROXY-prefixed and Unix cleartext H1 do the same;
- [ ] TLS ALPN H1 delegates to direct H1 as before;
- [ ] Auto H2 prior knowledge still resolves to the core H2 path;
- [ ] core no longer constructs or drives a Hyper HTTP/1 connection;
- [ ] historical public H1 compatibility entry points remain source-compatible;
- [ ] no detached shutdown-forwarder task remains per accepted connection;
- [ ] shutdown receiver/future lifetime is bounded by connection lifetime;
- [ ] repeated short connections do not accumulate forwarder receivers/tasks;
- [ ] connection permits/gauges remain balanced;
- [ ] topology gate rejects a future second core H1 execution path;
- [ ] no API, capability, default, feature-name, or support-tier change.

## Non-goals

Do not move H2 into `eggserve-server`, make the direct server H2-capable,
redesign `Service`, replace the compatibility lifecycle API, remove
`eggserve-core`, change H2/H3 support tiers, introduce a new cancellation
dependency, or perform unrelated connection-pipeline cleanup.
