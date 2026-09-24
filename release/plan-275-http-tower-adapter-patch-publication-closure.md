# Plan 275 — HTTP/Tower adapter patch publication closure

**Disposition: UNBLOCKED.** The corrected core patch is published and a clean,
registry-only Axum consumer passed against the published direct server.

## Release candidate and publication

- Implementation candidate: `5105a63d1d1569c4646c05c8d6fd96e83efe71ea`.
- Final repository status/evidence commit before this record:
  `67ef0a95e2c1a1419bbd5a1fa4d6ea2ba5b0417f`.
- Plan 274 implementation: `e49d67b3a459a11686a20a9c13cb183fc2a1dbd4`.
- Plan 274 hosted CI: run [`35954240517`](https://github.com/eggstack/eggserve/actions/runs/35954240517), passed.
- Plan 275 hosted CI on the exact release candidate:
  run [`35959464673`](https://github.com/eggstack/eggserve/actions/runs/35959464673), passed (Rust, Python, and supply-chain jobs).
- Live registry preflight on 2026-09-24 found `eggserve-core 0.2.1` latest
  and `0.2.2` unused; `eggserve-server 0.2.1` was already published.
- Changed Rust publish set: `eggserve-core` only. The server, primitives,
  static, TLS, H3, and binary crates were not republished.
- `scripts/verify-cargo-packages.sh --mode all` passed for the staged local
  registry; `cargo publish -p eggserve-core --locked --dry-run` passed on the
  clean release candidate.
- `cargo publish -p eggserve-core --locked` published `eggserve-core 0.2.2`.
  crates.io recorded the version at `2026-09-24T11:55:54.295973Z` with
  checksum `d7ad027bf61c2f505c5a591348d03ab7d92cf1849af45e8d804e5593eb13bb54`.
  The version is not yanked.

## Registry-only consumer

The consumer was created under `/tmp/eggserve-plan275-consumer-0.2.2`, outside
the EggServe workspace, with a fresh lockfile and no `[patch]`, path, or git
dependency entries. Its direct manifest was:

```toml
[dependencies]
eggserve-core = { version = "=0.2.2", default-features = false, features = ["tower"] }
eggserve-server = { version = "0.2", default-features = false }
axum = { version = "0.8", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
bytes = "1"
futures-util = "0.3"
http = "1"
```

The consumer's Cargo metadata and lockfile resolved every EggServe package
from `registry+https://github.com/rust-lang/crates.io-index`:

| Crate | Resolved version | crates.io checksum |
| --- | --- | --- |
| `eggserve-core` | `0.2.2` | `d7ad027bf61c2f505c5a591348d03ab7d92cf1849af45e8d804e5593eb13bb54` |
| `eggserve-server` | `0.2.1` | `255c1e95d0e6c6f7db8c26ea128267b3a1851dafe0f13a8229cdbec38a630feb` |
| `eggserve-primitives` | `0.2.0` | `8732e77fae06b395f992cb1dd163ca30750e2f44d1fed3824eb8fc4650dafc03` |
| `eggserve-static` | `0.2.0` | `3079735e246a37f49830aef10ed01de9aa70c0fb59a19f2aac5a99b513046a61` |

Other principal resolved versions were Axum `0.8.9` and Tokio `1.53.1`.
Cargo confirmed the actual published features and the consumer compiled the
public `TowerToEggserve<axum::Router>` bound directly, without a local wrapper.
The complete consumer lockfile is retained at
[`plan-275-registry-consumer-Cargo.lock`](plan-275-registry-consumer-Cargo.lock)
(SHA-256 `57e84a813122bd77b66b0c06fdd7ace6b029e693874074027461c15612f32e98`)
for all resolved versions and registry checksums.

Run from the external consumer directory:

```sh
CARGO_TARGET_DIR=/tmp/eggserve-plan275-consumer-0.2.2/target \
  cargo test --manifest-path /tmp/eggserve-plan275-consumer-0.2.2/Cargo.toml --locked
```

Result: **1 test passed**. Deterministic channels verified that the first
Axum response chunk reached the socket before the gated second chunk was
released. The same live server proved a chunked request reached the handler
before request completion, duplicate response header order survived, middleware
ran, disconnect dropped the streaming producer, and external
`ServerControl::shutdown()` was followed by a clean typed
`ServerCompletion::wait()` result.

No PyPI release was performed or required. Python runtime behavior is
unchanged. The repository roadmap and release metadata now record 0.2.2 as
published, and Plan 275 is formally complete.
