# eggserve-primitives

`eggserve-primitives` is the canonical transport-neutral application model
(Plan 214). It owns validated request/response/header/body/lifecycle values,
policy value types, proxy/connection metadata, and neutral tunnel intent —
with no transport, async-runtime, TLS, QUIC, Hyper, or filesystem/platform
dependencies.

> Authority: values live here; H1 execution lives in `eggserve-server`;
> static/path/FS/MIME/planning implementation lives in `eggserve-static`;
> `eggserve-core::primitives` / `eggserve_core::layers` are compatibility
> facades. Guard: `scripts/check-crate-topology.py`.

## Manifest

- Crate root: `crates/eggserve-primitives/src/lib.rs` — single
  `pub mod primitives` plus glob re-export.
- Production deps (`Cargo.toml`): `bytes`, `futures-util` only.
  `http-interop = []` is an accepted empty compatibility name; the crate
  pulls no HTTP dependency for it (interop adapters live in
  `eggserve-server::interop`, core forwards).
- Dev deps: `proptest`, `tempfile`, `tokio` (test-only runtime).

## Module inventory (`src/primitives/`)

| Module | Key types |
|--------|-----------|
| `method` / `version` / `authority` | `Method`, non-exhaustive `HttpVersion` (`Http10`/`Http11`/`Http2`/`Http3`), `Authority` |
| `header_block` | `HeaderBlock`, `HeaderField`/`Name`/`Value`, `HeaderValueTextError` |
| `request_target` | `RequestTarget`, `RequestTargetForm`, `RequestTargetError` (sole target classifier) |
| `request_head` / `request` / `request_context` | `RequestHead`, `Request`, `RequestContext` (`connection` + `lifecycle` + `interim` + `tunnel_request`) |
| `connection_info` / `proxy` | `ConnectionInfo`/`Scheme`/`SocketEndpoints`/`TlsInfo`, `TrustedProxyConfig`/`IpPrefix`/`ProxyProtocolConfig`/`ForwardedConfig`, effective/provenance layer (Plan 202) |
| `request_body` / `request_body_policy` / `request_body_error` / `incomplete_body_policy` | one-shot `RequestBody`, `RequestBodyPolicy`, 14-variant `RequestBodyError`, `IncompleteBodyPolicy` |
| `request_lifecycle` | `RequestLifecycle`, `RequestCancellationReason`, body lifecycle state |
| `interim` / `trailers` | bounded 1xx `InterimSender`/`Limits`, `Trailers`/`TrailerLimits` (Plan 198) |
| `tunnel` | neutral intent vocabulary `TunnelRequest`/`Kind`, classifiers/validators (Plans 199/216; execution in `eggserve-server::tunnel`) |
| `http` | `ReadOnlyMethod`, `validate_method`/`body`/`target`, `RequestValidationError` (static/read-only helpers) |
| `policy` | `StaticPolicy`, `SymlinkPolicy`, serve-level `DotfilePolicy`, `DirectoryListingPolicy`, `StaticMetadataPolicy`, `ErrorRepresentationPolicy` |
| `limits` | `Limits`, `LimitsError` (shared bound authority) |
| `response` | planning values `StaticResponsePlan`/`ResponseStatus`/`HeaderMapPlan`/`BodyPlan`, `FileRange` (private fields; `try_new`/`new` + accessors), conditional/range outcomes |
| `body` | `BodySource`/`BodyKind`/`BodySourceError` (handle-carrying streaming) |
| `response_stream` | `ResponseStream`/`Error`, `MAX_RESPONSE_STREAM_CHUNK_BYTES`, trailer/length constructors |
| `canonical` (+ `status`/`headers`/`response_body`/`response`) | `StatusCode`, `ResponseHead`, `ResponseBody`, `BodyLength`, `Response`/`Builder`, `NormalizeRequest`, `normalize_response`/`normalize_metadata`, `is_hop_by_hop_header`, `runtime_error_with_policy` |

## Ownership boundaries

- Owns types, validation, and normalization — never socket I/O, accept loops,
  shutdown, Hyper conversion (`eggserve_server::adapters::to_hyper_response`
  is the single framing authority), filesystem opens, or MIME tables.
- `StaticPolicy::safe_default()` denies listing/symlinks/dotfiles with
  standard validators. Parse-level `eggserve_static::path::DotfilePolicy`
  vs serve-level `policy::DotfilePolicy` must both agree.
- Response framing belongs to the runtime: `normalize_response` /
  `normalize_metadata` converge all producers; EggServe owns `Date` by
  default (direct H1 can explicitly transfer successful service metadata);
  1xx/204/205/304 are body-forbidden (304 keeps representation
  length); `BodyLength::Unknown` never becomes `Content-Length: 0`.
- Plans 280/282/283 add server-side embedding policy (`PolicyOwner`,
  `H1ConnectionPolicy`, rejection presenter) without moving any
  primitives-owned type.

## See also

- [eggserve-server](eggserve-server.md) / [eggserve-static](eggserve-static.md) — runtime and static authorities.
- [primitives-api](primitives-api.md), [policy-system](policy-system.md), [response-planning](response-planning.md) — deep dives.
- Normative: [docs/http-primitives.md](../docs/http-primitives.md), [docs/http-response-planning.md](../docs/http-response-planning.md), [docs/secure-root.md](../docs/secure-root.md).
