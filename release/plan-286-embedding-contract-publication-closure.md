# Plan 286 — Embedding-contract publication closure

## Qualified source and tunnel decision

Plan 285 selected `eggserve-server 0.3.0` because exhaustive `RuntimeConfig`
literals and direct users of the service/tunnel semaphore accessors have
source-incompatible changes. Plan 284 retained the direct opaque H1 tunnel
transport after same-host in-process Tokio duplex A/B and correctness evidence.
Its measurement limits remain material: no TCP loopback, isolated CPU/RSS, or
manual cross-platform qualification is claimed.

Proof-bearing hosted CI source candidate: `c62faf59b19913eb49b97d371435122c5a8fb6ac`, CI run `36067050590` (Rust, Python wheel, and supply-chain jobs all passed). A follow-up lockfile-only correction selected the unused next CLI patch `eggserve-bin 0.2.1`; its exact-SHA hosted run `36068742416` on `02aa50c14bf30c57422c40b1fbf8afc87a528a38` also passed all jobs.

Plan 277 publication and Plan 279 registry-only closure were consolidated into this release per maintainer direction. No standalone Plan 277 `0.2.3` candidate was published. The compatible direct Tower API is included in the `eggserve-server 0.3.0` artifact.

## Published package set

Published serially in dependency order; every `cargo publish --locked` completed successfully and Cargo confirmed registry availability before dependent publication.

| Package | Version | Published UTC | crates.io checksum |
| --- | --- | --- | --- |
| `eggserve-primitives` | `0.2.1` | 2026-09-24 22:40:42.124968 | `ba5372af39cb279fab9cc672608fe83c3ac5ca16d8f8ce058c2400626ef3a101` |
| `eggserve-server` | `0.3.0` | 2026-09-24 22:40:52.622500 | `b26bcaeb357dfafeb780649c789765c7b6545d85388ecdbb446a47ff78aac082` |
| `eggserve-static` | `0.3.0` | 2026-09-24 22:41:01.807562 | `70f140261c246784464dcb95d1bdf67cb3f68e4f7a77a37db881d3831accf45e` |
| `eggserve-h3` | `0.3.0` | 2026-09-24 22:41:13.252399 | `6c22ed28b2ed901cf0bc5d5f5d2fa11cd5369bf56fe05ca794f4303266bad7c5` |
| `eggserve-core` | `0.3.0` | 2026-09-24 22:41:21.403241 | `564ee8f4b5a2dbc6b4fe3a36edec27694eb4831a2849833545bf277785ccc1b6` |
| `eggserve-bin` | `0.2.1` | 2026-09-24 22:41:30.797359 | `f50ab99762170a07beac43d3243c63a1cc56985e2a3ef44c6b410aa8a265b17d` |

The version selection was checked against live crates.io state on 2026-09-24. The previously published baselines were primitives `0.2.0`, server `0.2.1`, static `0.2.0`, H3 `0.2.0`, core `0.2.2`, and bin `0.2.0`. `eggnet-tls` remained unchanged at `0.2.0`; no Python distribution was published.

`bash scripts/verify-cargo-packages.sh --mode all`, the pinned supply-chain checks for both lockfiles, and a `cargo publish --locked --dry-run` for primitives passed before publication. Each actual upload also completed Cargo's package verification against the now-visible prerequisite artifacts.

## Registry-only consumer proof

All fixtures are standalone workspaces under `release/fixtures/plan-286-*`, pin exact EggServe artifact versions, have committed lockfiles, and contain no path, git, or patch override. Each was first resolved and built using a fresh `CARGO_HOME`; locked reruns and release-profile test builds passed.

| Consumer | Exact EggServe artifacts | Test result | Lock packages | `cargo tree -e no-dev` nodes | Release test executables (bytes) |
| --- | --- | --- | ---: | ---: | --- |
| default | primitives `0.2.1`, server `0.3.0` | defaults + tunnel suite: 11 passed | 33 | 30 | 1,197,272; 3,190,312 |
| advanced | primitives `0.2.1`, server `0.3.0` | H1/TLS embedding 13 passed; tunnel suite 10 passed | 68 | 30 | 7,037,920; 3,190,368 |
| Tower/Axum | primitives `0.2.1`, server `0.3.0` with `tower` | Axum streaming 1 passed; interop 16 passed | 45 | 41 | 3,361,472; 3,231,696 |
| core compatibility | core/static/server `0.3.0`, primitives `0.2.1` | facade 1 + static-default 1 passed | 58 | 62 | 1,211,896; 3,564,432 |
| core Tower forwarding | same, with core `tower` feature | facade/static 2 + Tower forwarding 1 passed | 58 | 64 | 1,214,424; 3,564,168; 3,461,200 |

The default consumer asserts origin-only target parsing, EggServe-owned deadline/ceiling/admission defaults, and active parser bounds. Its tunnel suite proves Upgrade/CONNECT behavior, read-ahead preservation, default tunnel admission, and shutdown.

The advanced consumer negotiates TLS ALPN `http/1.1` in caller-owned Rustls/Tokio-Rustls and passes the established TLS stream to the published direct H1 API. It covers external handler/body/idle/write deadline and body/target ceiling ownership, service/tunnel admission ownership, custom typed rejection presentation, absolute-form metadata and bounds, duplicate canonical header fields in order, request streaming/trailers, tunnel handoff, and shutdown. Connection-total timeout enabled and disabled forms are exercised. Core, static, and PHF are absent from the direct server no-dev graph.

The Tower consumer proves Axum composition, streaming request/response bodies, trailers, duplicate headers, cancellation/lifecycle behavior, and adapter conversion tests. Its no-dev graph has no core/static/PHF ancestry. The core consumer compiles historical compatibility facade paths, runs the published static service and confirms dotfiles remain denied by default, and runs the forwarded Tower adapter against a real loopback H1 request.

The byte measurements are sizes of release-profile integration-test executables, not production binary size or performance claims. Profiles and exact artifacts are reproducible with the fixture manifests and lockfiles.

## Registry dependency measurements

The direct native graph resolves 30 no-dev package nodes; direct Tower resolves 41. The direct graphs contain no `eggserve-core`, `eggserve-static`, or `phf`. The full lock package counts include test-only dependencies. Exact manifests, lockfiles, tests, full `cargo metadata` JSON, and `cargo tree -e no-dev` snapshots are retained in the four fixture directories; regenerate metadata with:

```sh
cargo metadata --locked --format-version 1 --manifest-path release/fixtures/plan-286-advanced/Cargo.toml
cargo tree --locked -e no-dev --manifest-path release/fixtures/plan-286-advanced/Cargo.toml
cargo metadata --locked --format-version 1 --manifest-path release/fixtures/plan-286-tower/Cargo.toml --features tower
cargo tree --locked -e no-dev --manifest-path release/fixtures/plan-286-tower/Cargo.toml --features tower
```

## Closure and downstream status

Plan 279's deferred registry-only closure is satisfied by the published default and advanced consumers. EggReplay M013B's EggServe publication blocker is cleared: the downstream can now qualify against `eggserve-primitives 0.2.1` and `eggserve-server 0.3.0`. No downstream repository changes were made or assumed.

Plans 278–286 are closed at their stated scopes. Plan 277 itself remains unchanged; its candidate was folded into this published artifact set. Remaining qualification limits are the documented Plan 284 benchmark boundary and separately required manual platform qualification.
