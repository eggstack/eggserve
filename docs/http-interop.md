# HTTP / Tower Interoperability (Plan 200)

Optional, feature-gated adapters connect EggServe's canonical model to the
Rust HTTP ecosystem without replacing it. Native `Service` remains the
authoritative, maximum-fidelity path; these adapters are explicit edges for
middleware and application stacks.

Enable with Cargo features (never in default builds):

```toml
eggserve-core = { version = "0.1", features = ["http-interop"] }  # http + http-body adapters
eggserve-core = { version = "0.1", features = ["tower"] }         # + Tower adapters
```

- `http-interop` adds a direct `http` dependency and the
  `primitives::interop` module.
- `tower` adds `http-interop` plus `tower-service` / `tower-layer` and the
  `server::tower` module. Full `tower` is never required; layers compose via
  `tower-layer` only.

## When to use what

1. **Native `Service`** for maximum fidelity and control: exact header order,
   raw target bytes, one-shot interim/tunnel capabilities, and the normative
   7-stage commitment contract in
   [downstream-app-server.md](downstream-app-server.md).
2. **`http` / `http-body` adapters** (`primitives::interop`) for ecosystem
   message/body compatibility: convert heads, stream request bodies as
   `http_body::Body`, and accept `http_body` response bodies into the
   canonical pipeline.
3. **Tower adapters** (`server::tower`) for middleware/application stacks:
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
- **Metadata**: `ConnectionInfo`, effective `Authority`, and `RequestLifecycle`
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

- `RequestBody: http_body::Body` yields `Frame::data` then one optional
  `Frame::trailers` after content completion. One-shot ownership, truthful
  `size_hint` (remaining declared bytes, not a post-failure guarantee),
  sanitized errors, drop preserves abandoned-body semantics, cancellation
  wakes polls. No unbounded buffering.
- `response_from_http_body(http::Response<B>)` streams `B` incrementally
  into `ResponseStream` (data + validated trailers, no full buffering).
  Application `content-length`/`transfer-encoding` are stripped;
  normalization recomputes framing. `HEAD`/body-forbidden never poll.
- `response_from_bytes` / `empty_response_from_http` cover common cases
  without exposing `ResponseBody` variants.

## Transports

Adapters are transport-independent: the same Tower application runs on H1
today and on H2/H3 through the same canonical kernel where those features
are enabled. Qualification fixtures prove H1 over TCP
(`tests/interop_http_tower.rs`); H2/H3 reuse the existing runtime
qualification plus the shared body/timeout accounting.

## Fidelity rule

Do not make Tower or `http` the protocol-correctness authority. Where a
standard type cannot express a property exactly, keep it in EggServe metadata
or fail with `InteropError` — never silently coerce.
