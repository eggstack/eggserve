# HTTP / Tower Interoperability (Plan 200)

Optional, feature-gated adapters connect EggServe's canonical model to the
Rust HTTP ecosystem without replacing it. Native `Service` remains the
authoritative, maximum-fidelity path; these adapters are explicit edges for
middleware and application stacks.

Use the direct profile for H1 application services, or the compatibility
profile when composing static serving or the multiprotocol runtime. Both
adapter features remain opt-in:

The direct-server adapters are published in `eggserve-server 0.3.0` with
registry-only consumer proof (see
`../release/plan-286-embedding-contract-publication-closure.md`). Depend on
the published leaves; the staged-package validation uses these direct imports.

```toml
eggserve-server = { version = "0.3", default-features = false, features = ["http-interop"] }
eggserve-server = { version = "0.3", default-features = false, features = ["tower"] }
# Compatibility/static/multiprotocol composition:
eggserve-core = { version = "0.3", features = ["tower"] }
```

`eggserve-server` owns the interop and Tower implementations. The historical
`eggserve_core::primitives::interop` and `eggserve_core::server::tower` paths
remain compatibility re-exports. Core continues to depend on
`eggserve-static` by design; direct H1 + Tower consumers do not need core.

- `eggserve-server/http-interop` adds the direct `http` dependency and the
  `eggserve_server::interop` module.
- `eggserve-server/tower` adds `http-interop` plus `tower-service` and
  `eggserve_server::tower`. Full `tower` is never required. Tower layers
  compose around the adapters, but the feature does not activate
  `tower-layer` (Plan 296: production adapter code never implements the
  `Layer` contract); downstream `Layer` authors depend on `tower-layer`
  directly.
- Core's same-named features forward to the direct server features.

## When to use what

1. **Native `Service`** for maximum fidelity and control: exact header order,
   raw target bytes, one-shot interim/tunnel capabilities, and the normative
   7-stage commitment contract in
   [downstream-app-server.md](downstream-app-server.md).
2. **`http` / `http-body` adapters** (`eggserve_server::interop`) for ecosystem
   message/body compatibility: convert heads, stream request bodies as
   `http_body::Body`, and accept `http_body` response bodies into the
   canonical pipeline.
3. **Tower adapters** (`eggserve_server::tower`) for middleware/application stacks:
   run a Tower service on EggServe (`TowerToEggserve`) or expose a native
   service as Tower (`EggserveToTower`).

## Round-trip limitations

- **Header order**: `HeaderBlock` preserves exact field-line order and
  duplicates. `http::HeaderMap` preserves duplicates per name via `append`
  but not global cross-name order. Consumers needing wire order use native.
- **Request target**: only origin-form (`/path?query`) round-trips.
  Absolute/authority/asterisk forms are rejected, never normalized. Exact
  bytes survive in `RawTargetExt`.
- **Opaque values**: map through `HeaderValue::from_bytes`, never mandatory
  UTF-8.
- **Metadata**: `ConnectionInfo` (including Plan 202 provenance-tagged
  effective fields when an explicit trusted-proxy policy adopted them),
  effective `Authority`, and `RequestLifecycle`
  travel in typed extensions (`ConnectionInfoExt`, `AuthorityExt`,
  `LifecycleExt`). Interim senders and one-shot tunnel capabilities never
  enter `Extensions` (clonable); they stay in native `RequestContext`.

## Middleware boundary

Tower layers operate **after** EggServe parsing/validation and **before**
final normalization. Middleware may add ordinary headers/content but cannot
bypass body hard limits, framing validation, the header denylist/privacy
policy, no-progress timeouts, or lifecycle/shutdown. `max_in_flight_requests`
stays an outer hard ceiling; Tower readiness only further gates app work.

- `TowerToEggserve` clones the Tower service per request and drives
  `poll_ready` on that clone. No shared `Mutex` serializes requests; shared
  state lives behind `Clone` (typically `Arc`).
- `EggserveToTower::poll_ready` is always ready (adapter-local only, never
  transport admission).

## Bodies

- `RequestBody` converts to `HttpRequestBody`, the explicit server-owned
  `http_body::Body` adapter. It yields `Frame::data` then one optional
  `Frame::trailers` after content completion. One-shot ownership, truthful
  `size_hint` (remaining declared bytes, not a post-failure guarantee),
  sanitized errors, drop preserves abandoned-body semantics, cancellation
  wakes polls. No unbounded buffering.
- `response_from_http_body(http::Response<B>)` streams `B` incrementally
  into `ResponseStream` (data + validated trailers, no full buffering).
  Application `content-length`/`transfer-encoding` are stripped;
  normalization recomputes framing. Bodies reporting an exact size hint are
  declared known-length so `Content-Length` is emitted; the transport
  verifies the count and fails closed on mismatch (Plan 295).
  `HEAD`/body-forbidden never poll.
- `response_from_bytes` / `empty_response_from_http` cover common cases
  without exposing `ResponseBody` variants.

## Transports

Adapters are transport-independent: the same Tower application runs on H1
today and on H2/H3 through the same canonical kernel where those features
are enabled. Qualification fixtures prove H1 over TCP
(`crates/eggserve-server/tests/interop_http_tower.rs`); H2/H3 reuse the existing runtime
qualification plus the shared body/timeout accounting. Plan 207 records this
mapping in `conformance/app_server_conformance.toml` and proves H1 + H2/Tower-gated parity in `tests/cross_protocol_conformance.rs` (see `release/plan-207-cross-protocol-conformance.md`).

## Fidelity rule

Do not make Tower or `http` the protocol-correctness authority. Where a
standard type cannot express a property exactly, keep it in EggServe metadata
or fail with `InteropError` — never silently coerce.
