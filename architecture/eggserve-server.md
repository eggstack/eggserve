# eggserve-server

`eggserve-server` is the Plan 215 mature H1 runtime layer. It owns the
per-runtime observability vocabulary (`ops`: events, sinks, counters,
`OpsContext`), the `ServerError`/`ShutdownResult` taxonomy, the response
privacy policy (`ResponsePolicy`/`DatePolicy`/denylist validation), the
shared runtime-limit authority (`runtime_limits`: defaults, `Violation`,
`SharedRuntimeValues`), the application `Service` contract shape
(`Service`/`ServiceError` with `Internal`/`Rejected`/`Panic`/`Timeout`,
`service_fn`/`service_fn_head`/`service_fn_with_policy`), the connection
vocabulary (`ConnectionContext`/`ConnectionShutdown`/`ConnectionOutcome`),
H1 configuration/state (`config::RuntimeConfig` + builder,
`runtime::RuntimeState`), the H1 connection driver (request conversion, body
policy, admission, panic containment, handler/body/header/idle/write/total
timeouts, max-requests handling, normalization, Hyper conversion via
`adapters`), and the listener/prebound TCP `Server` (`bind`,
`from_listener`/`from_std_listener`, accounted accept loop, per-connection
shutdown relay, `wait()`/`ops_snapshot()`). It depends on
`eggserve-primitives` and transport dependencies (`bytes`, `futures-util`,
`http-body`, `http-body-util`, `httpdate`, `hyper` http1/server,
`hyper-util`, `tokio`).

It has no dependency on `eggserve-core` or `eggserve-static`, so a downstream
application server can select the runtime without inheriting static-file
confinement or MIME implementation code. The crate is strict HTTP/1: H2
selection, PROXY-preamble reading, and extended TLS identity stay
compatibility-owned (Plans 217/202/203); inbound tunnel acceptance is
direct-owned (Plans 199/216, inbound-only per Plan 223 — no outbound
CONNECT dialing, no CONNECT-crate or client-stack dependency); H3 mechanics live once in `eggserve-h3` (Plan 220) over a small
shared kernel exposed here (`connection::select_body_policy`,
`contain_service_panic`, `invoke_canonical_service`,
`finalize_canonical_response`, lifecycle registry; H3 `Alt-Svc` stays
H3-owned, no Quinn types here). The `http2`/`tls` Cargo features remain as
inert opt-in edges (the H1 graph never requires them).

`eggserve-core::server` remains the 0.1 compatibility surface for those
advanced paths, with facades (`ops`, `errors`, `response_policy`, `policy`,
`runtime_limits`) over every moved module. Unified `Service` identity and
tunnel-acceptance convergence landed in Plan 216 and single-contract convergence in Plan 217 (downstream fixture). The direct crate The direct crate
is the preferred generic H1 substrate and is not a promotion of H2/H3 or
tunnel functionality.

Parity evidence: `crates/eggserve-core/tests/direct_h1_parity.rs` (16
scenarios, wire-for-wire against the compatibility pipeline),
`crates/eggserve-server/examples/caller_owned.rs` (downstream-neutral
embedding demo), and the topology ownership rules in
`scripts/check-crate-topology.py`. See
`release/plan-215-direct-runtime-parity.md` for the ownership matrix,
criteria accounting, and migration notes.
