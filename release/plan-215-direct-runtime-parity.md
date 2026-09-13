# Release record — Plan 215 Direct Embeddable Connection Runtime Parity

Implemented 2026-09-13. `eggserve-server` is now the implementation home of
the mature generic HTTP/1 connection-serving substrate; the compatibility
`eggserve-core::server` keeps its extended (TLS/H2/H3/tunnel/proxy-accept)
ownership with facades over every moved module.

## Source ownership before/after

| Module | Before | After |
|---|---|---|
| `ops` (events/sinks/counters/context) | `eggserve-core/src/ops/` | `eggserve-server/src/ops/`; core `ops/mod.rs` re-exports |
| `ServerError`/`ShutdownResult` | `core::server/errors.rs` | `eggserve-server/src/errors.rs`; core re-exports |
| `ResponsePolicy`/`DatePolicy`/denylist | `core::server/response_policy.rs` | `eggserve-server/src/response_policy.rs`; core re-exports |
| `ErrorRepresentationPolicy` et al | `core::policy.rs` (nominal duplicate) | `eggserve_primitives::policy`; core `policy.rs` re-exports (duplicate deleted) |
| `SharedRuntimeValues`/`Violation`/constants | `core::runtime_limits.rs` (`pub(crate)`) | `eggserve-server/src/runtime_limits.rs` (public); core re-exports; `From` adapters live next to `Limits`/`RuntimeConfig` |
| `Service`/`ServiceError`/`service_fn*` | simplified direct def + mature core def | mature hyper-free def in `eggserve-server/src/service.rs` (Panic/message/`is_panic`/`is_timeout`/`From<RequestBodyError>`/`ServiceFn`); core keeps its Hyper-coupled definition pending Request-type unification (see exceptions) |
| `ConnectionContext`/`ConnectionShutdown`/`ConnectionOutcome` | `core::server/connection/context.rs` | `eggserve-server/src/connection/context.rs`; core keeps its tunnel-capable copy (see exceptions) |
| H1 `RuntimeConfig`/`Builder`/`Http1Config` | `core::server/config/` (+ tiny direct stub) | `eggserve-server/src/config.rs`; core keeps extended (TLS/H2/H3) config projecting onto the shared kernel |
| `RuntimeState` | `core::server/runtime.rs` | `eggserve-server/src/runtime.rs` (file/service/reserved-tunnel semaphores + ops) |
| H1 connection kernel (`request`/`response`/`activity`/`lifecycle`/`transport`/`deferred_body`/`driver` H1/`pipeline` H1) | `core::server/connection/` | `eggserve-server/src/connection/`; H2 selection, H3 paths, and tunnel acceptance stay in core |
| Hyper conversion boundary | `core::primitives/canonical/adapters.rs` (664 lines) | `eggserve-server/src/adapters.rs` (+ `src/response.rs` error builders) |
| `to_hyper_response` overloads, `runtime_error_with_policy` | core-only | direct primitives widened (`runtime_error_with_policy`, `into_parts`, `ByteStream`/`TrailerFuture`, `RequestShared`/`BodyLifecycleState`, wire-slot/body-sharing APIs now public adapter surface) |
| Direct `Server` convenience runtime | simplified inline driver (dropped accept errors, fabricated no metadata) | listener/prebound `Server` driving the canonical driver with observed socket metadata, accounted accept errors, per-connection shutdown relay |

## Direct crate dependency graph before/after

Before: `eggserve-primitives`, `bytes`, `futures-util`, `http-body`,
`http-body-util`, `hyper` (http1/server), `hyper-util`, `tokio`
(macros/net/time/io-util/fs/sync), `thiserror`.

After: same plus `httpdate` (sole `Date` authority + `Last-Modified`
comparison) and `tokio/rt` (deferred-body supervision tasks); `thiserror`
removed (no remaining use). No `eggserve-core`, `eggserve-static`, H3/QUIC,
Tower, `http`, filesystem, or application dependencies. Default/H1 graph
unchanged in kind: still no TLS/H2/H3/static/Tower.

## Direct vs compatibility H1 conformance

New suite `crates/eggserve-core/tests/direct_h1_parity.rs` (16 tests)
drives identical raw-HTTP scenarios through both stacks and asserts
identical status/head (minus `Date` value), body bytes, and outcome:

- valid GET/HEAD (HEAD keeps `Content-Length`, zero wire bytes)
- bodies in Reject (413) / Buffer-echo / Stream-echo modes
- chunked bodies and chunked trailers (`5|true` echo both sides)
- malformed framing (400, `ClientError` both sides)
- request-target (414) and header-byte (431) ceilings
- handler timeout (504), stalled-body collapsed timeout (504),
  header timeout (`header-timeout` outcome), max-requests-per-connection
  (`Connection: close` + `normal`)
- service panic (500, no detail leak either side)
- invalid H1 configs rejected with byte-identical messages (zero
  connections, handler>total cross-field, hand-constructed invalid)

Result: 16/16 pass. During development the stalled-body case initially
assumed 408; both stacks returned 504 (collapsed body/handler timeout
disambiguation after the timed-out future drops the body), confirming the
port preserves even the subtle timeout semantics.

Caller-owned lifecycle/shutdown evidence (direct crate):
`connection::tests::presignaled_token_terminates_caller_owned_driver`
(`Shutdown` outcome), `Server` smoke tests (bind port 0, prebound
`from_std_listener`), and `examples/caller_owned.rs` (downstream accept +
pre-handoff reject + truthful `for_tcp` metadata + shutdown + outcome).

## Real socket metadata propagation

`Server::start_with_service` builds `ConnectionContext::for_tcp` from
observed `stream.local_addr()`/`accept` peer addresses (no fabrication;
prebound listeners report their actual bound address). Proven by
`tcp_server_reports_real_socket_metadata` (service observes the real client
port and bound address) and the parity suite's address-carrying paths.

## Anti-duplication/topology gate output

`scripts/check-crate-topology.py` extended with Plan 215 rules:

- server source (incl. examples) never references `eggserve-core/static`
- single-definition markers owned by server (`OpsContext`, `EventKind`,
  `ServerError`, `ResponsePolicy`, `SharedRuntimeValues`, `Service`,
  `ServiceError`, `ConnectionContext`, `RuntimeConfig`, `RuntimeState`,
  `serve_http1_connection*`, `drive_connection`, `hyper_builder`,
  `make_canonical_hyper_service`, `invoke_service`, `to_hyper_response`)
- core facades re-export (`ops`, `server::errors`,
  `server::response_policy`, `policy`, `runtime_limits`)
- service-contract shape parity (Panic/`is_panic`/`is_timeout`/message/
  `service_fn_with_policy`/`service_fn_head` in both definitions)

`python3 scripts/check-crate-topology.py` →
"Plan 211–215 topology: ... direct H1 runtime owns
ops/errors/policy/authority/service/driver".
`python3 scripts/verify-conformance-matrix.py` → 51 matrix entries, 55
app-server scenarios (47 routine), 17 H3 scenarios validated.

## Intentional pre-1.0 migration notes

- `eggserve_server::service_fn` now returns `ServiceFn<F>` (was the bare
  closure); both implement `Service`, so `start_with_service(service_fn(..))`
  keeps compiling. New: `service_fn_head`, `service_fn_with_policy`.
- `ServiceError::rejected` takes `(status, message)` (was `(status)`).
  New: `Panic` category, `message()`, `is_panic()`, `is_timeout()`,
  `status_code()`, `From<RequestBodyError>`.
- `eggserve_server::{RuntimeConfig, ServerError, Server, ServerBuilder,
  ServerHandle}` replaced by the mature shapes (`config::RuntimeConfig`
  with the H1 field set, `errors::ServerError` 10-variant taxonomy,
  listener/prebound `Server` with `wait()`/`ops_snapshot()`).
- `eggserve_core::policy` and `eggserve_core::runtime_limits` are now
  re-exports (nominal duplicates deleted); `Limits::validate` and
  `RuntimeConfig::{validate,try_from_serve_config}` route through the
  server-owned kernel with identical messages.
- Direct primitives widened adapter surface: `RequestShared`,
  `BodyLifecycleState`, `WireTrailerSlot`/`new_wire_slot`, body-sharing
  constructors/observers, `into_parts`, `runtime_error_with_policy`.

## Acceptance accounting and Plan 216 handoff

Held: 1 (H1 kernel; tunnel branch inert with seam comments), 2, 3, 4, 6
(non-tunnel H1 behavior, proven by the parity suite), 7, 8, 10, 11, 12.

Explicitly deferred with documented reason (Plan 216 input):

- Criterion 5 (one `Service`/`ServiceError` definition): the definitions
  are behaviorally converged with a topology gate enforcing shape parity,
  but not yet identical. Identity requires Request-type unification, which
  requires moving the tunnel capability slot (`RequestContext::tunnel`,
  `take_tunnel_acceptance`, `admit_and_spawn`) into direct types — that is
  generic tunnel extraction, owned by Plan 216. Forcing identity now would
  either regress Plan 199 tunnel behavior or redesign its service contract
  without 216's design review.
- Criterion 9 (core H1 paths as re-exports): holds for `ops`, `errors`,
  `response_policy`, `policy`, and `runtime_limits`. The tunnel-capable
  core pipeline, core `Service`, core `ConnectionContext`, and the
  Hyper-coupled conversion helpers remain by the same reason; every
  remaining core H1 module is either a facade or tunnel/H2/H3-owned.
- No binary-size claims are made (no measured artifacts).
