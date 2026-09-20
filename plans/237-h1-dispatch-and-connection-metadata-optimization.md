# Plan 237 — H1 dispatch and connection-metadata optimization

## Prerequisite

Plan 234 must characterize the remaining H1 dispatch layer, ordinary zero-tunnel
driver path, and established-TLS metadata cost.

## Purpose

Remove internal H1 dynamic-dispatch/refcount/synchronization costs left after
Plan 228 without changing the public `eggserve_server::Service` contract,
driver entry points, timeout semantics, or request metadata API.

## Track A — generic CanonicalHyperService

Replace the remaining internal:

```text
CanonicalHyperService
  Arc<dyn Fn(Request<Incoming>) -> Pin<Box<dyn Future<...>>>>
```

with a named generic internal Hyper service holding:

```text
CanonicalHyperService<S>
  state: Arc<PipelineState<S>>
```

and implementing `hyper::service::Service` directly.

Requirements:

- keep `PipelineState<S>` as the single per-connection shared immutable
  handle;
- one `Arc<PipelineState<S>>` clone per request is acceptable;
- preserve `Send + 'static` requirements needed by Hyper/upgrades;
- retain one boxed future if Rust/Hyper bounds make it the cleanest design;
- do not contort the implementation solely to remove the final box;
- do not change `ServiceFuture`, `Service::call`, or
  `Service::call_with_tunnel`;
- direct H1 and compatibility H1 continue through the same canonical pipeline.

Benchmark the removal of the outer `Arc<dyn Fn>` independently from any
attempt to remove future boxing.

## Track B — zero-tunnel driver fast path

The ordinary H1 driver should not take an async mutex merely to establish that
no tunnel has ever been created if Plan 234 confirms that this occurs in the
normal deadline loop.

Introduce the minimum state needed to bypass `JoinSet` locking in the common
zero-tunnel case, for example an atomic active count or monotonic
"tunnels-ever-spawned" flag.

Requirements:

- the `JoinSet` remains the ownership/drain authority;
- spawn and completion accounting cannot race to an incorrect zero;
- shutdown/total-timeout drain semantics remain exact;
- no detached tunnel survives `wait()`;
- no new polling loop;
- H2/H3 shared tunnel semantics are not silently changed.

If the existing lock is not material in Plan 234, close this track NO-GO.

## Track C — immutable connection metadata sharing evaluation

Established connections carry immutable raw endpoints/scheme/TLS metadata.
Today request construction can deep-clone string/certificate-bearing metadata.

First inventory the crate-boundary constraints. Prefer a private representation
change in `RequestContext` or another existing neutral primitive over an
additive supported API.

An implementation may proceed only if it can preserve:

- `RequestContext::connection() -> &ConnectionInfo`;
- existing public constructors;
- `Request::connection()`;
- owned `Request::into_parts*` behavior;
- trusted-forwarding derivation and provenance;
- public `ConnectionInfo` and `TlsInfo` field types;
- caller-owned transport construction.

Strong rule: do not add a new public supported constructor solely to pass an
`Arc<ConnectionInfo>` across crates. A narrowly documented hidden
runtime-adapter bridge is permissible only if repository conventions already
support that pattern and qualification shows a meaningful benefit. Otherwise
record DEFER.

A copy-on-write style is acceptable: ordinary requests may share a connection
template, while trusted forwarding that changes effective metadata may
materialize request-specific state.

## Tests

- direct H1 parity fixture;
- caller-owned H1;
- upgrades/CONNECT and tunnel shutdown/drain;
- max-requests/idle/write/total timeout behavior;
- trusted and untrusted forwarding;
- TLS metadata access including opt-in peer certificate chain;
- H2 compatibility/service convergence where shared primitives are touched.

## Measurement

Against Plan 234:

- custom 1 KiB c1/c16/c64;
- established TLS 1 KiB keep-alive;
- allocation/refcount profile around request dispatch;
- zero-tunnel ordinary requests;
- one accepted tunnel for correctness/resource cost.

Retain Track A when it removes an avoidable dynamic layer with equal or simpler
code and no regression even if timing is near noise. Tracks B/C require
measured or mechanically significant resource reduction.

## Non-goals

- No public Service future redesign.
- No custom Hyper fork or executor.
- No weakening timeout/activity accounting.
- No TLS session semantic changes.
- No general extension/type map in RequestContext.

## Acceptance criteria

- [ ] The outer `Arc<dyn Fn>` dispatch layer is removed or a documented type
      constraint explains why it must remain.
- [ ] Public Service and connection-driver APIs are unchanged.
- [ ] Zero-tunnel locking is reduced only if evidence supports it.
- [ ] Metadata sharing lands only with a clean API-neutral boundary; otherwise
      it is explicitly deferred.
- [ ] Direct/compatibility/tunnel/TLS/proxy tests pass.
- [ ] Same-machine evidence records each KEEP/NO-GO decision.
