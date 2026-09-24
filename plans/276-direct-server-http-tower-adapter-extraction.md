# Plan 276 — Direct-server HTTP/Tower adapter extraction and static-free downstream profile

## Purpose

Separate EggServe's optional `http`/Tower ecosystem adapter from the
`eggserve-core` compatibility/static umbrella so direct H1 application-server
consumers can compose `eggserve-server` with Tower/Axum without pulling
`eggserve-static`, PHF, or core's unrelated static-serving facade.

The motivating downstream is EggPool, but the implementation must remain a
generic EggServe package-boundary improvement. EggPool-specific routes,
configuration, LLM semantics, retry logic, or lifecycle policy must not enter
EggServe.

Planning baseline:

```text
5c41141 docs: close Tower adapter publication plan
```

Current published baseline recorded by Plans 274–275:

- `eggserve-core 0.2.2` is published with the repaired Tower adapter;
- `eggserve-server 0.2.1` is the published direct H1 runtime;
- `eggserve-primitives 0.2.0` and `eggserve-static 0.2.0` remain compatible
  leaf releases.

Related work:

- Plan 200 — optional `http` / `http-body` / Tower interoperability;
- Plans 214–225 — direct-crate authority split and compatibility facade;
- Plans 270–273 — direct-server supervision and downstream embedding;
- Plans 274–275 — repaired and published the current core-owned Tower adapter;
- EggPool Plans 246–248 — production EggServe adoption and footprint evidence.

## Problem statement

The current `eggserve-core` manifest declares `eggserve-static`
unconditionally:

```toml
eggserve-static = { path = "../eggserve-static", version = "0.2.0" }
```

and `eggserve-static` owns the compile-time MIME table:

```toml
phf = { version = "0.11", features = ["macros"] }
```

This is correct for `eggserve-core`'s role as the compatibility/composition
umbrella: its semver-considered primitives and static facades expose
`ConfinedPath`, `SecureRoot`, MIME/static planning, and related policy.

It is not an ideal dependency boundary for a direct H1 application server that
needs only:

```text
eggserve-server runtime
  + canonical request/response primitives
  + http/http-body conversion
  + TowerToEggserve
```

EggPool demonstrates the mismatch. Its production integration uses
`eggserve-server` for the listener/runtime and `eggserve-core` only for
`TowerToEggserve` plus the body policy path. Plan 248 measured the migration
from its pre-EggServe baseline as +9 lockfile packages. The newly introduced
set includes the direct runtime/primitives, but also the compatibility/static
side:

```text
eggserve-core
eggserve-static
phf
phf_generator
phf_macros
phf_shared
siphasher
```

Those seven packages are not required by the intended direct H1 + Tower
composition itself. The exact downstream removal count must be re-measured
after implementation rather than assumed, because a consumer may independently
already contain one of those packages.

## Architecture decision

Do **not** solve this by making `eggserve-static` optional inside
`eggserve-core`.

Core's no-default feature profiles are already a tested public composition
surface, and its primitives/static facade references the static authority
throughout. Making the static dependency conditional would require broad
`cfg` fragmentation or change the meaning of existing
`default-features = false` consumers.

Do **not** create a new `eggserve-tower` / `eggserve-interop` micro-crate
for this correction unless implementation proves the server boundary cannot
own the adapter without violating an existing invariant.

The preferred ownership is:

```text
eggserve-primitives
        ^
        |
eggserve-server
  - H1 runtime / Service authority
  - response transport adapter
  - optional http-interop
  - optional Tower adapter
        ^
        |
eggserve-core
  - compatibility/composition umbrella
  - static facade remains unconditional
  - http/Tower paths become re-exports of server-owned authority
```

This follows the post-Plan-217 direct architecture: the Tower adapter exists to
adapt an ecosystem service into `eggserve-server::Service`, and its inverse
adapter exposes that same Service contract back to Tower. The direct server
already owns the canonical-to-Hyper response conversion used by the inverse
adapter.

The default `eggserve-server` graph must remain Tower-free. Adapter
dependencies stay opt-in.

## Track A — Freeze the current graph and package evidence

Before source movement, record:

```bash
cargo tree -e no-dev -p eggserve-core --no-default-features --features tower
cargo tree -e features -p eggserve-core --no-default-features --features tower
cargo tree -e no-dev -p eggserve-server
cargo metadata --format-version 1
```

Confirm and retain as evidence:

1. current core `tower` consumption reaches `eggserve-static`;
2. `eggserve-static` reaches PHF;
3. default direct `eggserve-server` does not depend on core/static;
4. the server already owns the public response transport conversion needed by
   the inverse Tower adapter.

Also record the current package/normal-node counts for a tiny Axum consumer
using the published/core-shaped adapter. This becomes the comparison baseline;
do not use EggPool's entire repository graph as the only measurement.

## Track B — Move HTTP interop ownership to `eggserve-server`

Move the implementation currently owned by
`crates/eggserve-core/src/primitives/interop.rs` into a direct-server
ecosystem adapter module, preferably:

```text
crates/eggserve-server/src/interop.rs
```

The server-owned module must expose the same generic concepts required by the
Tower adapter:

- `HttpRequestBody` — server-owned legal newtype over canonical
  `eggserve_primitives::RequestBody`;
- `RequestBodyHttpError`;
- `InteropError`;
- request-head conversions;
- response-from-`http_body::Body` conversion;
- connection/lifecycle/raw-target extension types;
- header/trailer conversion helpers that are already public on the current
  interop surface.

Use direct `eggserve_primitives` imports. Do not route the implementation
back through `eggserve-core`.

Add optional server feature ownership:

```toml
[features]
default = []
http-interop = ["dep:http"]
tower = ["http-interop", "dep:tower-service", "dep:tower-layer"]
```

Exact dependency expressions may differ if Cargo shows an already-required
dependency can remain non-optional. Keep the intent:

- `tower-service` / `tower-layer` must not enter the default server graph;
- no `eggserve-static` or PHF dependency may enter `eggserve-server`;
- no Axum production dependency;
- `eggserve-primitives` remains transport-neutral and does not acquire
  `http`, Tower, Hyper, Tokio, or static dependencies.

Preserve all Plan-274 behavior: one-shot body ownership, data/trailers, size
hints, limit errors, lifecycle/cancellation, opaque header bytes, duplicate
header semantics, and sanitized errors.

## Track C — Move Tower adapter ownership to `eggserve-server`

Move the implementation currently in
`crates/eggserve-core/src/server/tower.rs` to:

```text
crates/eggserve-server/src/tower.rs
```

behind the new server `tower` feature.

The moved implementation must use only direct authorities:

- `eggserve_primitives` for canonical request/response/context vocabulary;
- `crate::service::{Service, ServiceError}` for the native contract;
- `crate::adapters::to_hyper_response` for canonical response transport
  conversion;
- `crate::interop` for the standard-http bridge;
- Tower traits only behind the feature.

Preserve the current public adapter set and semantics:

- `TowerToEggserve`;
- `EggserveToTower`;
- `TowerAdapterError`;
- per-request service cloning and `poll_ready`;
- explicit `RequestBodyPolicy`;
- incremental response conversion;
- no global mutex;
- no transport-admission claim from Tower readiness.

For direct-consumer ergonomics, add only narrow re-exports justified by the
service boundary. In particular, it is reasonable for
`eggserve-server` to re-export `RequestBodyPolicy` beside its existing
`Request` re-export so a consumer can write:

```rust
use eggserve_server::{
    RequestBodyPolicy,
    tower::TowerToEggserve,
};
```

Do not turn the server root into a broad primitives facade.

## Track D — Preserve `eggserve-core` compatibility paths as pure facades

Core remains the compatibility/composition umbrella and continues to depend on
`eggserve-static` unconditionally. This plan is not a core-static redesign.

Change core's adapter features to forward into the server authority:

```toml
http-interop = ["eggserve-server/http-interop"]
tower = ["http-interop", "eggserve-server/tower"]
```

Convert the current core-owned implementation files to thin compatibility
re-exports, preserving existing source paths:

```text
eggserve_core::primitives::interop::...
eggserve_core::server::tower::...
eggserve_core::server::{TowerToEggserve, EggserveToTower, TowerAdapterError}
```

Expected shape:

```rust
pub use eggserve_server::interop::*;
pub use eggserve_server::tower::*;
```

Use the narrowest module arrangement that keeps rustdoc and feature-gating
truthful.

Remove core's direct optional `http`, `tower-service`, and `tower-layer`
dependency declarations if repository search/Cargo proves no remaining
production use. Do not remove unrelated Hyper/body dependencies used by core's
H2/TLS/compatibility implementation.

The core feature profiles from Plan 274 must continue to compile:

```bash
cargo +1.89 check -p eggserve-core --all-targets   --no-default-features --features http-interop

cargo +1.89 check -p eggserve-core --all-targets   --no-default-features --features tower
```

Core consumers are allowed to retain the static/PHF closure because core is
still the umbrella. The optimization comes from allowing downstream direct H1
consumers not to depend on core at all.

## Track E — Move tests to the new authority and keep compatibility smoke

Move implementation-specific interop/Tower tests to the server package so the
tests live with the authority.

At minimum server feature qualification must cover:

- `HttpRequestBody` data/trailer/size-hint behavior;
- body ownership and sanitized failure behavior;
- request-head and response conversions;
- `TowerToEggserve` readiness and request streaming;
- `EggserveToTower` response normalization and streaming;
- duplicate headers;
- middleware composition;
- cancellation/drop behavior.

Retain a small `eggserve-core` compatibility test that imports the old core
paths and proves they resolve to working adapters. Do not keep a second copy of
the behavioral suite in core.

The existing Axum 0.8 qualification should be changed so its primary path is:

```text
eggserve-server::Server
  -> eggserve_server::tower::TowerToEggserve
  -> axum::Router
```

with no production or test dependency on `eggserve-core` for the adapter.

Axum remains dev-only.

## Track F — Add durable topology and CI guards

Extend `scripts/check-crate-topology.py` so this separation cannot regress.

Required invariants:

1. `eggserve-server` must not depend on `eggserve-core` or
   `eggserve-static`;
2. the server's Tower feature must not introduce `eggserve-static`, `phf`,
   `phf_generator`, `phf_macros`, `phf_shared`, or `siphasher`;
3. `eggserve-primitives` remains free of `http`/Tower/Hyper/Tokio/static
   production dependencies;
4. core `http-interop`/Tower source files are facade-only after extraction;
5. the default server build does not enable Tower traits/layers.

Update routine CI and `scripts/verify.sh fast` with focused direct-server
adapter gates:

```bash
cargo +1.89 check -p eggserve-server --all-targets   --no-default-features --features http-interop

cargo +1.89 check -p eggserve-server --all-targets   --no-default-features --features tower

cargo clippy -p eggserve-server   --no-default-features --features tower   --lib --tests -- -D warnings

cargo test -p eggserve-server   --no-default-features --features tower
```

Keep the existing core adapter gates as compatibility-facade gates. Do not
replace one blind spot with another.

Avoid an all-feature combinatorial explosion.

## Track G — EggPool-shaped external consumer qualification

Create or update a clean external fixture outside the workspace that uses only
the direct package path intended for EggPool:

```toml
[dependencies]
eggserve-server = {
  version = "<local/staged candidate>",
  default-features = false,
  features = ["tower"]
}
axum = { version = "0.8", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
```

Add only support dependencies genuinely required by the fixture. If
`RequestBodyPolicy` is re-exported from `eggserve-server`, the fixture must
not add a direct primitives dependency merely for convenience.

The fixture must contain no `eggserve-core` dependency.

Prove:

1. pre-bound `TcpListener` ownership;
2. `RuntimeConfig::disable_connection_total_timeout()`;
3. `TowerToEggserve::with_policy(axum::Router, ...)`;
4. finite request/response;
5. chunked or otherwise incremental request body delivery;
6. incremental `Body::from_stream` response before producer completion;
7. duplicate response headers;
8. middleware execution;
9. direct `ServerControl` + passive `ServerCompletion::wait()` shutdown;
10. client disconnect propagates to the application producer.

This is a generic Axum fixture. Do not reproduce EggPool routes or LLM
protocols.

## Track H — Measure the packaging win, do not assume it

Compare two clean external consumers on the same host/toolchain/profile:

A. current compatibility profile:

```text
eggserve-core --no-default-features --features tower
+ eggserve-server
+ Axum
```

B. extracted direct profile:

```text
eggserve-server --no-default-features --features tower
+ Axum
```

Record:

- lockfile package count;
- normal/no-dev dependency-node count;
- whether `eggserve-core` is absent in profile B;
- whether `eggserve-static` is absent in profile B;
- whether PHF-family packages are absent in profile B;
- release executable bytes for the identical fixture;
- optional clean-build wall time only as descriptive evidence, never a CI
  threshold.

Do not require a particular binary-size reduction. Rust dead-code elimination
may make dependency-graph savings larger than final linked-size savings.

A material *increase* in the direct profile requires explanation before
closure.

## Track I — Documentation and package-boundary reconciliation

Update current-authority docs after the source move:

- `architecture/crate-topology.md`;
- `architecture/eggserve-core.md` or the current core composition page;
- `architecture/runtime.md` if it enumerates server-owned adapters;
- `docs/dependency-policy.md`;
- `docs/http-interop.md`;
- `README.md` Rust embedding guidance where appropriate;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`.

Document two distinct Rust consumption profiles:

```text
direct H1 application service:
  eggserve-server (+ optional tower/http-interop)

compatibility/static/multiprotocol composition:
  eggserve-core
```

Do not describe `eggserve-core` as defective for pulling static serving; that
is its intended umbrella role. The defect is requiring that umbrella for a
direct adapter that can live on the direct service authority.

## Required qualification

Focused first:

```bash
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --no-default-features --features tower

cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features tower
cargo clippy -p eggserve-core --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower

python3 scripts/check-crate-topology.py
```

Then the repository's current routine gates, including:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
```

Use the exact current repository commands at execution time.

## Acceptance criteria

- [ ] `eggserve-server` owns the only implementation of standard-http body
      interop used by the Tower adapter.
- [ ] `eggserve-server` owns the only implementation of
      `TowerToEggserve` / `EggserveToTower`.
- [ ] The server adapter is opt-in; the default server graph gains no Tower
      dependency.
- [ ] `eggserve-server --features tower` has no dependency on
      `eggserve-core`, `eggserve-static`, or PHF.
- [ ] `eggserve-primitives` remains transport-neutral.
- [ ] Existing `eggserve_core::primitives::interop::*` source paths compile.
- [ ] Existing `eggserve_core::server::tower::*` and root server adapter
      re-exports compile.
- [ ] Core's static compatibility surface remains unchanged.
- [ ] Axum 0.8 qualifies through the direct server-owned adapter.
- [ ] Incremental request/response streaming and cancellation semantics remain
      unchanged.
- [ ] Direct control/completion supervision remains unchanged.
- [ ] Topology/CI gates permanently cover both the server authority and core
      compatibility facade feature profiles.
- [ ] A clean direct consumer contains no `eggserve-core` or
      `eggserve-static` dependency.
- [ ] A clean direct consumer's EggServe path contains no PHF-family package.
- [ ] Package/dependency/binary comparisons are recorded without unsupported
      performance claims.
- [ ] No new adapter micro-crate is added unless this plan is amended with
      concrete evidence that server ownership is invalid.
- [ ] No EggPool-specific behavior enters EggServe.

## Non-goals

- No change to static serving behavior.
- No attempt to make `eggserve-static` optional inside `eggserve-core`.
- No new generic application framework or router.
- No Axum production dependency.
- No H2/H3 support-tier promotion.
- No TLS behavior change.
- No direct-server lifecycle redesign.
- No Python behavior change.
- No EggPool source change.
- No crates.io publication in this plan; publication/registry proof is Plan
  277.

Plan 276 ends when the source tree, tests, topology gates, and staged external
consumer prove that the Tower/Axum composition can be consumed directly from
`eggserve-server` without the compatibility/static umbrella. Registry
publication is owned by Plan 277.
