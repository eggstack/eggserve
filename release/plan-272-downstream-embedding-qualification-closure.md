# Plan 272 downstream embedding qualification closure

**Disposition: UNBLOCKED.** Both required 0.2.1 Rust packages are published,
and a fresh registry-only consumer resolves `eggserve-server 0.2.1` and passes
the supervised loopback keep-alive proof.

## Implementation provenance

- Planning baseline: Plans 270/271 were registered at `100b33c`.
- Implementation candidate: `466cf6301f20c7202f696c495e6eb8d5e74664be`
  (`feat: qualify supervised direct server embedding`). Later planning and
  closure commits did not change runtime source.
- Plan 270 adds `ServerHandle::into_parts()`, returning cloneable
  `ServerControl` and single-owner `ServerCompletion`. Completion reports
  typed terminal failures and its borrowed wait is cancellation-safe; legacy
  `wait(self) -> ()` remains source-compatible.
- Plan 271 gives `Duration::ZERO` an explicit meaning: disable only the total
  connection lifetime. The default remains 60 seconds, and independent
  request, idle, write, admission, and shutdown limits remain active.
- The existing 0.2.0 direct APIs remain source-compatible. The 0.2.1 patch is
  additive.

## Qualification provenance

- Plan 272 full source/API/security/package qualification completed on the
  0.2.1 candidate. It includes the combined leaf-crate fixture at
  `crates/eggserve-server/tests/downstream_embedding.rs`, feature and
  regression qualification, and package dry-runs.
- Routine CI run `35808907965` passed its Rust, Python, and supply-chain jobs
  for the implementation candidate.
- `scripts/verify-cargo-packages.sh` derives package versions from Cargo
  metadata; the layered local-registry package verification passed.
- On 2026-09-23, `cargo publish -p eggserve-server --locked --dry-run` and
  `cargo publish -p eggserve-core --locked --dry-run` both passed before
  publication.
- The current checkout's local verification passed on 2026-09-23 with
  `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 bash scripts/verify.sh fast`; this
  includes workspace tests, H2/TLS and H3/TLS core/bin lint and test lanes,
  formatting, and the excluded Python crate check. CI release workflow,
  wheel-matrix, and release-wheel-set validators/self-tests passed, as did the
  three Rust 1.89 workspace checks and TLS-only `eggserve-bin` Clippy/tests.
  Pinned cargo-audit/cargo-deny and `scripts/check-supply-chain.sh` passed for
  both lockfiles (advisories, bans, licenses, and sources).
  `scripts/check-python-release-metadata.py`,
  `scripts/check-crate-topology.py`, `scripts/verify-conformance-matrix.py`,
  and `scripts/verify-cargo-packages.sh --mode all` also passed.
- Candidate SHA above identifies runtime implementation. Publication and
  planning metadata were completed in later commits; they did not modify the
  qualified runtime.

## Publication provenance

The sparse crates.io index confirmed neither 0.2.1 existed immediately before
publication; latest versions were 0.2.0. The verified minimal changed Rust
publication set was used, in dependency order:

| Crate | Version | Published (UTC) | crates.io checksum |
|---|---:|---|---|
| `eggserve-server` | 0.2.1 | 2026-09-23 11:56:24 | `255c1e95d0e6c6f7db8c26ea128267b3a1851dafe0f13a8229cdbec38a630feb` |
| `eggserve-core` | 0.2.1 | 2026-09-23 11:56:33 | `52e0abd7a104ddfa035bc83aca03aeb935c7b841135cc2d9eb8abbd47974d552` |

The server version was registry-resolvable with `cargo info` before core was
published. No unrelated crates were republished. The server checksum is also
in the consumer lockfile; the core checksum is from the crates.io index.

## Registry-only consumer proof

The temporary consumer was created outside this repository at
`/tmp/eggserve-273-consumer`. Its manifest was:

```toml
[package]
name = "eggserve-registry-proof"
version = "0.1.0"
edition = "2021"

[dependencies]
eggserve-server = { version = "0.2", default-features = false }
eggserve-primitives = { version = "0.2", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
```

There were no path dependencies, git dependencies, copied EggServe source,
or `[patch.crates-io]` entries. A fresh lockfile was generated from crates.io.
It resolves `eggserve-server 0.2.1` with checksum
`255c1e95d0e6c6f7db8c26ea128267b3a1851dafe0f13a8229cdbec38a630feb`, and
`eggserve-primitives 0.2.0` with checksum
`8732e77fae06b395f992cb1dd163ca30750e2f44d1fed3824eb8fc4650dafc03`.

The adapted supervised fixture passed using:

```sh
cargo tree --manifest-path /tmp/eggserve-273-consumer/Cargo.toml
cargo metadata --manifest-path /tmp/eggserve-273-consumer/Cargo.toml --format-version 1
cargo run --manifest-path /tmp/eggserve-273-consumer/Cargo.toml --locked
```

The tree and metadata resolve the published 0.2.1 server. The one execution
pre-binds a Tokio `TcpListener`, adopts it with `ServerBuilder::from_listener`,
uses `disable_connection_total_timeout()`, serves two requests on one healthy
keep-alive connection separated by 80 ms (past the configured 40 ms comparison
lifetime), splits the handle into control and completion, selects completion
against an external shutdown future, retains and uses `ServerControl`, and
observes `ShutdownResult::Clean`. The process exits normally with no detached
runtime task.

## Downstream disposition

**UNBLOCKED:** the published registry artifact provides the required direct
server API, and a clean registry-only consumer proves the supervised
keep-alive contract. Plans 270–273 are closed; Plan 273 reconciles the planning
record after publication and smoke completion.
