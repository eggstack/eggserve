# Plan 274 — HTTP/Tower request-body ownership corrective and Axum downstream qualification

## Purpose

Repair EggServe's advertised `http-interop` / `tower` integration surface after
the 0.2.1 release and prove the generic adapter against a real Axum 0.8
application without moving framework policy into EggServe.

This is an ownership and qualification corrective. It is motivated by the
EggPool downstream adoption gate, but the result must remain generic EggServe
composition infrastructure rather than an EggPool-specific adapter.

Planning baseline:

```text
091cddc release: close direct server downstream publication
```

Current published Rust baseline: `eggserve-core 0.2.1` and
`eggserve-server 0.2.1`.

Related work:

- Plan 200 — optional HTTP/http-body/Tower interoperability;
- Plans 215–225 — direct-crate authority split and compatibility-facade
  convergence;
- Plans 243–250 — direct H1 lifecycle/authority corrections;
- Plans 270–273 — direct-server supervision, unlimited total lifetime, and
  0.2.1 downstream publication;
- EggPool Plan 244 — intended EggServe/Tower/Axum transport composition;
- EggPool Plan 245 / commit `d492eade677acd6fc932c9a0c487b744a3070a91`
  — 0.2.0 Phase-0 stop evidence.

## Problem statement

The current 0.2.1 source still contains a structurally invalid implementation
in `crates/eggserve-core/src/primitives/interop.rs`:

```rust
impl http_body::Body for RequestBody {
    ...
}
```

Plan 217 moved the canonical `RequestBody` implementation into
`eggserve-primitives`. The core path is now only a compatibility re-export.
`http_body::Body` is owned by the external `http-body` crate, while
`RequestBody` is owned by the external-to-core `eggserve-primitives` crate.
Therefore `eggserve-core` owns neither side of that impl and the orphan rule
rejects the feature build.

This is not a downstream version-selection problem. It cannot be repaired by
an EggPool feature combination, compiler version, or Cargo patch that leaves
the source unchanged.

The repository already has substantial interop coverage in
`crates/eggserve-core/tests/interop_http_tower.rs`, but that target is guarded
by:

```rust
#![cfg(feature = "tower")]
```

Routine CI and `scripts/verify.sh fast` currently exercise the default
workspace plus H2/TLS and H3/TLS feature combinations. They do not enable
`http-interop` or `tower`. As a result, the tests intended to guard this
surface are not compiled in the ordinary validation path.

The fix must address both failures:

1. restore legal type ownership for the `http_body::Body` adapter;
2. make the advertised optional feature combinations first-class routine
   compile/test gates so the surface cannot silently regress again.

## Architecture constraints

Do not reverse the crate-ownership convergence to fix an adapter.

The following boundaries are mandatory:

- `eggserve-primitives` remains the transport-neutral canonical value/body
  authority and must not gain `http`, `http-body`, Tower, Axum, Hyper, or
  framework dependencies;
- `eggserve-server` remains the direct H1 runtime and `Service` authority;
- `eggserve-core` remains the compatibility/composition umbrella and is the
  correct home for optional ecosystem adapters;
- there must remain exactly one canonical `RequestBody` implementation;
- no second request-body state machine, trailer store, byte-limit tracker, or
  lifecycle allocation may be introduced;
- Axum may be a dev/qualification dependency only; production EggServe crates
  must not depend on Axum;
- the fix must not add routing, middleware, application state, or downstream
  process semantics to EggServe.

The preferred ownership shape is:

```text
eggserve-primitives::RequestBody
        |
        | owned canonical body
        v
eggserve-core::primitives::interop::HttpRequestBody
        |
        | local newtype implements external http_body::Body legally
        v
http::Request<HttpRequestBody>
        |
        v
TowerToEggserve
        |
        v
generic Tower/Axum application
```

## Track A — Replace the orphan impl with a core-owned HTTP body newtype

Add one small public adapter type under
`eggserve_core::primitives::interop`, preferably named
`HttpRequestBody`.

The exact name may change if a better concise name already fits repository
conventions, but the ownership semantics must not.

Target shape:

```rust
pub struct HttpRequestBody {
    inner: RequestBody,
}

impl From<RequestBody> for HttpRequestBody { ... }

impl HttpRequestBody {
    pub fn new(inner: RequestBody) -> Self { ... }
    pub fn into_inner(self) -> RequestBody { ... }
}

impl http_body::Body for HttpRequestBody {
    type Data = Bytes;
    type Error = RequestBodyHttpError;
    ...
}
```

Keep the field private. Do not duplicate canonical state.

The newtype's `Body` implementation must forward the current adapter semantics
exactly:

- data frames remain incremental;
- validated trailers appear at most once after content completion;
- canonical body/trailer failures map to the existing sanitized
  `RequestBodyHttpError` boundary;
- `size_hint` continues to expose truthful remaining declared bytes when
  available;
- `is_end_stream` accounts for pending terminal trailers;
- dropping the wrapper drops the same underlying canonical body, preserving
  `Active` / `Abandoned` / `Failed` behavior and connection-reuse safety;
- cancellation and body-progress wakeups continue through the existing
  `RequestLifecycle` allocation;
- no buffering, copying of the whole body, or secondary limit accounting is
  introduced.

Remove the illegal `impl http_body::Body for RequestBody` from core.

Do not solve this by moving the trait impl into `eggserve-primitives` and
adding `http-body` there. The neutral primitives dependency boundary is more
important than preserving a broken experimental adapter's exact request-body
type name.

## Track B — Rewire both Tower adapter directions

Update `server::tower` to use the legal wrapper consistently.

### TowerToEggserve

`TowerToEggserve<S>` should require a Tower service accepting:

```rust
http::Request<HttpRequestBody>
```

When a canonical EggServe request enters the adapter:

1. split the canonical request into head/body/context;
2. convert the head with the existing loss-aware interop path;
3. wrap the canonical `RequestBody` in `HttpRequestBody`;
4. preserve `ConnectionInfo`, authority, raw target, and lifecycle extensions;
5. drive per-request cloned Tower readiness as today;
6. convert the Tower response incrementally with
   `response_from_http_body`.

Do not introduce a shared mutex or framework-specific request conversion.

### EggserveToTower

The inverse adapter should implement Tower
`Service<http::Request<HttpRequestBody>>` and unwrap the adapter body back to
the same canonical `RequestBody` before constructing the native EggServe
`Request`.

Lifecycle selection must remain truthful: prefer the typed extension when
present, otherwise recover the lifecycle from the underlying canonical body.
Do not allocate a replacement lifecycle.

Keep existing response normalization/privacy/framing ownership unchanged.

## Track C — Preserve the non-Tower HTTP interop surface

`http-interop` must be independently useful and independently buildable.

Update `docs/http-interop.md` and Rust docs so they no longer claim that the
canonical `RequestBody` itself implements `http_body::Body`. Document the
explicit adapter instead:

```text
RequestBody
  -> HttpRequestBody
  -> http_body::Body
```

Retain the current scalar/head/header/response conversion APIs unless the
implementation proves a direct dependency on the illegal body impl.

Add focused tests under `http-interop` without requiring `tower` that prove:

- wrapping a fixed canonical body yields the expected data frame;
- a body with trailers emits data then exactly one trailer frame;
- size hints are preserved;
- adapter error display remains sanitized;
- `into_inner` returns the same canonical ownership object without cloning
  body state.

The standalone feature must pass with:

```sh
cargo +1.89 check -p eggserve-core --all-targets \
  --no-default-features --features http-interop
```

## Track D — Update the existing Tower qualification suite

Refactor `crates/eggserve-core/tests/interop_http_tower.rs` to use
`HttpRequestBody` rather than the canonical `RequestBody` as the Tower request
type.

Retain and re-prove the existing behavior:

- scalar conversions;
- opaque and duplicate headers;
- exact request target and connection extensions;
- request data/trailer streaming;
- response data/trailer streaming;
- framing-header distrust and final normalization;
- middleware-added ordinary headers;
- per-request Tower clone/readiness behavior;
- application-service error mapping;
- H1 transport parity.

Do not weaken tests merely to make the corrected type compile.

## Track E — Add a real Axum 0.8 downstream fixture

The generic Tower fixture is necessary but not sufficient. Add one small
framework-level qualification target proving the advertised adapter works with
Axum 0.8, which is the downstream shape that exposed the release defect.

Axum must be dev/test-only. Prefer:

```toml
[dev-dependencies]
axum = { version = "0.8", default-features = false }
```

Enable only additional dev features if the fixture actually requires them.
Do not add Axum to any production dependency graph.

The fixture should compose the public boundaries rather than reach into
private internals:

```text
caller-bound tokio::net::TcpListener
  -> eggserve_server::Server
  -> TowerToEggserve::with_policy(axum_router, bounded_stream_policy)
  -> Axum Router
```

Use the direct `eggserve-server` runtime rather than the compatibility-core
server so the fixture matches the current downstream supervision architecture
from Plans 270–273.

The Axum fixture must prove at minimum:

1. an `axum::Router<()>` satisfies the corrected
   `TowerToEggserve` request-body bound;
2. a chunked/unknown-length request reaches an Axum handler incrementally
   through the adapter;
3. an Axum `Body::from_stream` response reaches the socket incrementally;
4. the first response chunk is observable before a deliberately gated later
   chunk is produced, proving no whole-response collection;
5. duplicate same-name response headers retain their values/order semantics
   through the conversion boundary;
6. ordinary Axum middleware still executes inside the EggServe transport
   boundary;
7. client disconnect while streaming drops/cancels the application body
   rather than leaving an unbounded producer;
8. the direct server can still be supervised with
   `ServerHandle::into_parts()`, external shutdown through `ServerControl`,
   and typed `ServerCompletion`.

Use deterministic channels/barriers rather than timing-only sleeps for the
incremental-response assertion where possible.

This fixture is generic. Do not import EggPool, reproduce EggPool routes, or
encode LLM-specific behavior.

## Track F — Make optional interop features routine CI gates

The release defect persisted because advertised adapter features were outside
the ordinary validation matrix. Add explicit, small routine gates.

At minimum routine CI must run:

```sh
cargo +1.89 check -p eggserve-core --all-targets \
  --no-default-features --features http-interop

cargo +1.89 check -p eggserve-core --all-targets \
  --no-default-features --features tower

cargo clippy -p eggserve-core \
  --no-default-features --features tower \
  --lib --tests -- -D warnings

cargo test -p eggserve-core \
  --no-default-features --features tower
```

If standalone `http-interop` has feature-specific tests not reached through the
Tower lane, add its focused test command as well.

Mirror the same feature coverage in `scripts/verify.sh fast` so local handoff
and routine CI do not disagree.

Update `AGENTS.md` / `.opencode/skills/eggserve-dev/SKILL.md` command summaries
only as needed to keep the documented routine gate truthful.

Do not create a combinatorial all-features matrix. The goal is to permanently
cover the two advertised adapter entry features, not multiply CI cost.

## Track G — Feature/package topology requalification

Run `scripts/check-crate-topology.py` after the adapter change and extend its
rules only if needed to encode a durable ownership invariant.

The intended graph after this plan is:

- `eggserve-primitives`: unchanged neutral dependency class;
- `eggserve-server`: unchanged direct runtime dependency class;
- `eggserve-core/http-interop`: owns `http` / `http-body` adaptation;
- `eggserve-core/tower`: adds Tower traits/layers on top;
- Axum: dev-only test dependency.

Reject any implementation that makes Axum, Tower, or `http-body` flow into the
neutral primitives production graph.

Run `cargo tree -e features -p eggserve-core` for both adapter feature profiles
and record any unexpected production graph expansion.

## Track H — Documentation and compatibility accounting

Update current-authority docs to describe the legal wrapper:

- `docs/http-interop.md`;
- relevant `eggserve-core` Rust docs;
- `architecture/eggserve-core.md` or the current composition deep dive if it
  states `RequestBody: http_body::Body` directly;
- `AGENTS.md` / development skill only where the adapter contract or CI command
  list is stale.

Keep the stable `eggserve-core::primitives::RequestBody` facade unchanged.

The `server` and adapter surfaces are explicitly experimental. The broken
0.2.1 `tower`/`http-interop` feature path has no functioning source-compatible
consumer contract to preserve. Still document the request-type correction
clearly in the changelog/release notes for the next patch.

Do not promote the adapter support tier beyond its existing experimental
classification.

## Required qualification

Run the focused gates first:

```sh
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features tower
cargo clippy -p eggserve-core --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
```

Then run the repository's ordinary validation:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/check-supply-chain.sh
```

Use the exact current commands if repository policy changes during execution.

## Acceptance criteria

- [x] `eggserve-core --features http-interop` compiles on Rust 1.89.
- [x] `eggserve-core --features tower` compiles on Rust 1.89.
- [x] No orphan impl remains for the canonical `RequestBody`.
- [x] `eggserve-primitives` gains no HTTP/Tower/Axum dependency.
- [x] A core-owned HTTP request-body adapter preserves data, trailers, limits,
      size hints, errors, drop semantics, and lifecycle ownership.
- [x] `TowerToEggserve` and `EggserveToTower` consistently use the legal body
      adapter.
- [x] Existing generic Tower interop tests pass without weakening coverage.
- [x] A real Axum 0.8 Router composes with the direct
      `eggserve-server -> TowerToEggserve` path.
- [x] The Axum fixture proves request and response streaming remain
      incremental and bounded.
- [x] Duplicate headers and middleware behavior remain correct.
- [x] Direct control/completion supervision remains usable around the adapter.
- [x] Routine CI and `verify.sh fast` permanently compile/test the adapter
      features.
- [x] Default/no-feature, H2/TLS, H3/TLS, Python, topology, and supply-chain
      gates remain green.
- [x] No EggPool-specific runtime, route, configuration, or LLM policy enters
      EggServe.

## Non-goals

- No EggPool integration code.
- No replacement of Axum or Tower.
- No new router/application framework in EggServe.
- No HTTP/2 or HTTP/3 support-tier promotion.
- No change to direct-server supervision semantics from Plans 270–273.
- No static-serving behavior change.
- No new public timeout/configuration policy.
- No PyPI publication.
- No broad feature-matrix redesign.

Plan 274 ends when the source tree and routine CI prove the generic
`http-interop`/Tower/Axum composition. Registry publication is owned by
Plan 275.

## Execution status

**Complete — implementation qualified (2026-09-24).** Implementation
candidate: `e49d67b3a459a11686a20a9c13cb183fc2a1dbd4`. Local qualification
passed the focused Rust 1.89 interop/Tower checks, Tower Clippy and tests,
standalone interop tests, Axum qualification, topology checks, and
`scripts/verify.sh fast`. Hosted CI run
[`35954240517`](https://github.com/eggstack/eggserve/actions/runs/35954240517)
passed on that exact candidate SHA, including conformance, topology, supply
chain, Python, MSRV, H2/TLS, and H3/TLS lanes. Plan 275 is unblocked and ready
for release qualification; no other future plan dependency was found.
