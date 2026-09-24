# Plan 284 — TunnelIo transport-bridge optimization and ownership qualification

## Status

**CLOSED — KEEP; local A/B and tunnel correctness qualification plus exact-SHA hosted CI passed.**

Hosted CI: run 36067050590, SHA `c62faf59b19913eb49b97d371435122c5a8fb6ac` (success). The A/B evidence remains limited to in-process Tokio duplex, as recorded in the qualification artifact.

## Purpose

Determine whether the current generic tunnel handoff can remove its intermediate
32 KiB Tokio duplex + `copy_bidirectional` bridge without weakening
cancellation, read-ahead preservation, lifecycle tracking, or the public
no-Hyper abstraction.

The goal is lower copying/task/buffer overhead for long-lived H1 Upgrade and
CONNECT tunnels. Performance improvement is not assumed.

## Current design

Current accepted-tunnel flow is effectively:

```text
hyper::upgrade::Upgraded
  -> TokioIo
  -> copy_bidirectional
  -> 32 KiB DuplexStream
  -> public TunnelIo
  -> downstream tunnel handler
```

The bridge gives EggServe useful ownership properties:

- downstream code never sees Hyper;
- runtime can cancel/abort bridge + handler;
- read-ahead inside `Upgraded` is preserved;
- backpressure is bounded;
- tunnel tasks remain tracked for shutdown;
- no detached transport task survives connection completion.

Those invariants are more important than eliminating a copy.

## Phase A — Baseline before production change

Add a deterministic local tunnel benchmark/qualification harness before
changing implementation.

Measure at least:

- CONNECT-like bidirectional echo;
- Upgrade-like bidirectional echo;
- payload sizes approximately 1 KiB, 64 KiB, and 1 MiB;
- steady bidirectional transfer;
- connection setup/teardown churn;
- CPU time where available;
- throughput;
- p50/p95/p99 latency for fixed-message echo;
- peak/process RSS or another reproducible resident-memory proxy;
- task count or a direct structural accounting of spawned bridge/handler
  tasks.

Record compiler, target, host, profile, iteration count, and raw trial data.

The benchmark must be loopback/local and deterministic enough for same-machine
A/B comparison. Do not make CI performance-threshold dependent.

## Phase B — Candidate direct transport representation

Investigate an opaque `TunnelIo` implementation that can own the upgraded
transport directly.

A likely internal shape is a private trait-object/enum implementing
`AsyncRead + AsyncWrite + Unpin + Send`, e.g. a boxed runtime transport
wrapper. Public `TunnelIo` remains the only downstream type.

Requirements:

- no `hyper::upgrade::Upgraded` in a public signature;
- no `hyper_util::TokioIo<Upgraded>` in a public signature;
- downstream continues to receive one `TunnelIo`;
- `TunnelIo::pair()` test fixture remains available or has a compatible
  replacement;
- vectored-write capability is delegated truthfully;
- no unsafe code solely for transport erasure.

Do not introduce a new public generic parameter that forces application code
to name the transport.

## Phase C — Read-ahead preservation

Hyper may retain bytes read beyond the HTTP upgrade boundary.

The candidate must prove those bytes reach the tunnel handler exactly once,
in order, with no loss/duplication.

Add an adversarial test that sends:

```text
HTTP upgrade request headers


immediate post-upgrade application bytes in the same client write
```

The handler must receive the post-upgrade bytes intact.

Retain the corresponding baseline test for the existing bridge so A/B
semantics are directly comparable.

## Phase D — Cancellation and shutdown ownership

A direct transport handoff is acceptable only if EggServe can still terminate
an uncooperative/idle tunnel when:

- request lifecycle is cancelled;
- caller/server shutdown fires;
- total connection deadline fires when enabled;
- owning connection task is aborted after its drain budget.

Preferred model:

- handler still runs in a tracked task;
- the tracked JoinHandle is abortable by the runtime;
- abort drops `TunnelIo`, closing the underlying upgraded transport;
- connection completion drains/aborts tunnel tasks exactly as today.

Do not require the downstream handler to voluntarily poll a second cancellation
future merely to recover the current shutdown guarantee.

## Phase E — Commitment and admission invariants

Preserve:

- single accept;
- accept-after-commit rejection;
- validated H1 Upgrade/CONNECT classification;
- no body across tunnel transition;
- tunnel admission behavior from Plan 281, whether EggServe-owned or External;
- active-tunnel counters/gauges;
- no second HTTP response after tunnel commitment.

Transport optimization must not broaden which requests can become tunnels.

## Phase F — Error and observability behavior

Transport read/write failures must remain sanitized.

No payload bytes may enter logs/events.

Do not expose underlying Hyper error types to downstream handlers.

Keep existing tunnel accepted/closed/failure counters meaningful.

## Phase G — A/B decision

Run the exact same benchmark matrix against:

1. baseline bridge implementation;
2. candidate direct-transport implementation.

Retain the candidate only if at least one of the following is demonstrated
without material regression elsewhere:

- reproducible throughput/latency/CPU improvement;
- lower resident memory/task/buffer cost under many active tunnels;
- materially simpler ownership/lifecycle code with equal performance.

If the candidate adds unsafe complexity, cancellation ambiguity, or
non-reproducible performance, close **NO-GO** and retain the bridge.

Do not keep a more complex implementation for a marginal/noisy microbenchmark
win.

## Phase H — Cross-platform/API qualification

If retained:

- Linux/macOS/Windows compile/tests;
- Rust 1.89;
- existing tunnel conformance;
- direct server + core compatibility;
- optional Tower paths;
- no static/core dependency added to direct server;
- public `TunnelIo` API remains source compatible unless Plan 285 explicitly
  records otherwise.

If NO-GO:

- retain benchmark/evidence and document why the bridge remains the correct
  ownership tradeoff;
- no production source change is required.

## Acceptance criteria

- [x] baseline tunnel cost is measured before code change.
- [x] candidate preserves exact post-upgrade read-ahead bytes.
- [x] runtime can still force termination of idle/uncooperative handlers.
- [x] no raw Hyper transport/type becomes public.
- [x] admission/commitment/lifecycle semantics remain intact.
- [x] same-machine A/B evidence drives KEEP or NO-GO.
- [x] retained implementation has no meaningful performance/resource
      regression.
- [x] decision and raw evidence are recorded durably.
- [x] full CI is green for the retained state.

## Non-goals

- No WebSocket framing implementation.
- No generic proxy relay.
- No QUIC/H3 tunnel redesign.
- No io_uring/splice/sendfile.
- No unsafe transport casting.
- No publication in this plan.
