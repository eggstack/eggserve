# eggserve-server

`eggserve-server 0.3.1` is the single mature H1 runtime and transport
boundary (Plans 215–217, 243–250, 270–286). It owns the H1 driver,
`Service` contract, tunnel execution, per-runtime observability vocabulary,
response privacy, shared limit kernel, supervisory completion, external
policy/admission ownership, typed rejection presentation, opt-in
absolute-form dispatch, wider explicit parser ranges, external aggregate
header ownership, and successful service-response Date/Server ownership.
`eggserve-core` keeps compatibility facades only;
it owns H2/TLS/PROXY/listener composition, Unix/systemd/H3 paths, and
static orchestration.

Depends on `eggserve-primitives` plus transport deps (`bytes`,
`futures-util`, `http-body`, `http-body-util`, `httpdate`, `hyper`
http1/server, `hyper-util`, `tokio`) — never `eggserve-core`,
`eggserve-static`, PHF, `eggnet-tls`, or QUIC. `http2`/`tls` are inert
compatibility feature names; `http-interop`/`tower` are opt-in only.
Direct native graph has no core/static/PHF ancestry (Plan 286 registry
proof).

## Module inventory

| Module | Owns |
|--------|------|
| `adapters` | Outbound Hyper conversion; opaque `http_body::Body`, no `BoxBody` naming |
| `config` | `RuntimeConfig`/`RuntimeConfigBuilder`, `Http1RequestTargetMode`, `PolicyOwner`/`H1PolicyOwnership`, `AdmissionOwner`/`AdmissionOwnership`, `H1ConnectionPolicy`, rejection-presenter slot |
| `connection/` | H1 pipeline: `context` (`ConnectionContext`/`ConnectionShutdown`/`ConnectionOutcome`), `driver`, `pipeline`, `request`, `response`, `activity`, `transport` (`ProgressIo`), `lifecycle`, `deferred_body`; facade only exposes the driver + context |
| `errors` | `ServerError` (`#[non_exhaustive]`) / `ShutdownResult` single authority |
| `interop` (`http-interop`) | `HttpRequestBody`, head/response/trailer conversions over canonical types |
| `ops` | `OpsContext`/events/sinks/counters single authority |
| `rejection` | `RuntimeRejectionKind`/`RuntimeRejection`/`RuntimeErrorPresentation`/`RuntimeRejectionPresenter`, 64 KiB cap |
| `response` | Runtime-owned Hyper error responses (private detail) |
| `response_policy` | `ResponsePolicy`/`DatePolicy`/denylist single authority |
| `runtime` | `RuntimeState` admission pool (optional gates when externally owned) |
| `runtime_limits` | Shared defaults/validation kernel (`Duration::ZERO` = no total ceiling) |
| `service` | `Service`, `ServiceError`, `service_fn*`, tunnel-aware helpers |
| `tower` (`tower`) | `TowerToEggserve`/`EggserveToTower`/`TowerAdapterError`; per-request clones, adapter-local ready |
| `tunnel` | `TunnelCapability`/`TunnelIo` acceptance, bridging, drain |

## Service contract

One contract drives direct H1 (compatibility H2 uses the same canonical
types via core glue). `call(Request) -> Response` only (no `ServiceOutcome`,
no `poll_ready`); `request_body_policy()` defaults to `Reject`; `Send + Sync`.
Additive `call_with_tunnel(request, Option<TunnelCapability>)` defaults to
drop-and-`call`, so denial stays ordinary HTTP. Helpers: `service_fn`,
`service_fn_head`, `service_fn_with_policy`, `service_fn_with_tunnel`.
`RequestBodyPolicy` is re-exported for direct Tower composition. Panics
are contained to sanitized 500s; never a second error after commitment.

## H1 driver

`connection::serve_http1_connection` (+ `_with_id`, `+ _with_policy`) is the
sole H1 authority over any `AsyncRead + AsyncWrite` stream — no Hyper types,
no fabricated addrs. `Server::start_with_service` projects
`RuntimeConfig::h1_connection_policy()` once and drives accepted TCP through
the same driver with truthful socket contexts. Narrow `H1ConnectionPolicy`
carries only H1 deadlines/ceilings/admission/presenter/target-mode plus the
file-stream `stream_chunk_size`; bind, TLS-handshake, and listener
concurrency settings do not enter it. `Duration::ZERO` disables only total lifetime (default 60 s); parser
buffer/header-count/framing/header-timeout stay mandatory.

## Tunnel

Neutral vocabulary in `primitives::tunnel`; execution here (Plans 199/216,
223–224). `accept(headers, handler)` returns a handshake `Response` (101 H1 /
200 otherwise, runtime owns framing) plus bounded single-owner `TunnelIo`
(`AsyncRead + AsyncWrite`, 32 KiB bound, lifecycle-aware). Plan 284 KEEP:
opaque direct transport (`TunnelIoInner::Direct`) alongside the duplex
`Pair` fixture; read-ahead preserved exactly once; tracked drain on
shutdown. Inbound-only — no outbound CONNECT/client stack.

## Listeners / proxy metadata

Direct `Server`: `bind`, `from_listener`/`from_std_listener`, accounted
accept loop, per-connection shutdown relay, `ops_snapshot()`. Fixed 10 ms
accept-error backoff. Unix/systemd/H3/PROXY/TLS orchestration stays
core-owned; direct H1 keeps origin-form by default with header-forwarding
finalization only under explicit trusted-proxy policy (raw endpoints
preserved, H3 ignores).

## Interop / Tower (Plans 200/274/276/286)

Server owns the impl; core forwards `http-interop`/`tower` and keeps old
paths as facades. Default graph stays Tower-free; the enabled profile has no
static/PHF. `interop` is loss-aware (`from_bytes` opaque values,
cross-name order not round-tripped, interim/tunnel never in `Extensions`);
middleware runs post-validation, pre-normalization. Axum 0.8 qualified via
direct `TowerToEggserve` with no core dep.

## Supervisory completion (Plan 270)

`ServerHandle::into_parts() -> (ServerControl, ServerCompletion)`. Control is
cloneable shutdown-only (drop never stops the server). Completion is
single-owner; `wait(&mut self) -> Result<ShutdownResult, ServerError>` borrows
for `select!` reuse and maps top-level/escaping connection-task
panic/cancel to `ServerError::Terminal`. Drop requests graceful shutdown.
Legacy `wait(self) -> ()` stays source-compatible and discards detail.
Durable shutdown + `JoinSet` drain preserved (Plan 243).

## Policy projection (Plans 280–282)

`H1PolicyOwnership`: handler/body/idle/write deadlines + global body/target
ceilings independently EggServe- or externally-owned (default EggServe).
`AdmissionOwnership`: service/tunnel gates independently owned; external
means absent from `RuntimeState`, never a large sentinel. Externally-owned
decisions skip internal saturation counters; shutdown/lifecycle/drain stay
runtime-owned. A caller-owned TLS-stream fixture proves the combined contract
with no core/static deps.

## Typed rejection (Plan 283)

Synchronous `RuntimeRejectionPresenter::present(&RuntimeRejection) ->
Option<RuntimeErrorPresentation>`. Input is kind + runtime-selected status
only. Output cannot change status, framing, privacy, or disposition;
framing/`Date`/`Server` headers dropped. Panic/oversized/invalid output falls
back to generic representation. Hyper parser failures pre-conversion stay
outside the hook.

## Forward-proxy dispatch (Plan 278)

`Http1RequestTargetMode::OriginOnly` (default) vs `OriginOrAbsolute`
(explicit H1 opt-in). Opt-in validates scheme/authority, duplicate-Host
coherence, userinfo rejection, full-target `max_request_target_bytes`,
preserves scheme/authority/path/query + `form()` without Hyper types.
`RequestTarget::parse` stays origin-only; `ConfinedPath`/static stays
origin-only and rejects absolute-form. No proxy routing/outbound client;
CONNECT remains tunnel capability; no Python/CLI switch.

## Evidence

- `crates/eggserve-core/tests/direct_h1_parity.rs` (16 scenarios)
- `crates/eggserve-server/tests/downstream_embedding.rs` (supervision, policy, admission, presenter, proxy-form, tunnel)
- `release/plan-272-downstream-embedding-qualification-closure.md`
- `release/plan-278-forward-proxy-seam-implementation-closure.md`
- `release/plan-280-external-policy-ownership-closure.md`
- `release/plan-281-external-admission-ownership-closure.md`
- `release/plan-282-h1-connection-policy-projection-closure.md`
- `release/plan-283-typed-runtime-rejection-closure.md`
- `release/plan-284-tunnel-transport-ab-qualification.md`
- `release/plan-285-embedding-contract-qualification.md`
- `release/plan-286-embedding-contract-publication-closure.md` (+ `release/fixtures/plan-286-*/`)
- Guards: `scripts/check-crate-topology.py`, `scripts/verify-conformance-matrix.py`

## Authority notes (Plans 243/244/249/253, no ownership change)

Plan 243 makes shutdown durable — the listener `Server` keeps accepted
connection tasks in a runtime-owned `JoinSet` and drains them on shutdown.
Plans 244/249 establish the single-H1-authority corrective: compatibility
`Auto` classification resolves before any Hyper service exists and every H1
path delegates the replayable stream to `connection::serve_http1_connection`
here, while core executes H2 only (see
`../release/plan-250-h1-authority-lifetime-corrective-closure.md`). The
remaining core/server connection-module parallels are classified, not
duplicated authority — see the overlap ledger in
[crate-topology.md](crate-topology.md) (Plan 253).
