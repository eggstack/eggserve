# Plan 277 — Direct Tower adapter publication and registry-only static-free consumer closure

## Purpose

Publish the Plan-276 direct-server HTTP/Tower adapter extraction and prove from
crates.io that an EggPool-shaped Axum consumer can use the public
`eggserve-server` package without resolving `eggserve-core`,
`eggserve-static`, or the PHF MIME-table dependency family.

This is the release/evidence closure for Plan 276. It does not reopen the
Plan-274/275 adapter correctness work or the Plan-270/273 direct-server
lifecycle work.

Planning baseline:

```text
5c41141 docs: close Tower adapter publication plan
```

Depends on:

- Plan 276 — direct-server HTTP/Tower adapter extraction and static-free
  downstream profile.

Published baseline at planning time:

- `eggserve-core 0.2.2`;
- `eggserve-server 0.2.1`.

Repository source metadata is currently synchronized at 0.2.2. Because the
server package will gain the adapter feature/API and core will change to a
compatibility re-export/feature-forwarding facade, both packages are expected
to be in the changed publish set. Execution must derive the actual set and
query crates.io immediately before selecting a version.

## Release architecture

After Plan 276, the intended registry relationship is:

```text
eggserve-primitives
        ^
        |
eggserve-server <new patch>
  optional: http-interop, tower
        ^
        |
eggserve-core <new patch>
  compatibility/static umbrella
  forwards http-interop/tower to server
  retains static dependency by design
```

The direct consumer path is:

```text
eggserve-server --features tower
  -> eggserve-primitives
  -> http/http-body/Tower ecosystem deps
```

and must not contain:

```text
eggserve-core
eggserve-static
phf
phf_generator
phf_macros
phf_shared
siphasher
```

unless a test application's unrelated dependencies independently require one
of those crates. The evidence must identify dependency ancestry, not merely
grep a flat lockfile.

## Track A — Reconfirm the changed publish set and version availability

Immediately before release preparation:

1. query crates.io/live Cargo registry for `eggserve-server`,
   `eggserve-core`, and any other source-changed package;
2. identify the next unused synchronized 0.2.x patch;
3. derive the changed registry package set from the Plan-276 diff and
   `cargo metadata`;
4. do not publish unchanged packages solely for numerical symmetry.

Expected changed publish set:

1. `eggserve-server` — new optional adapter features/modules/API;
2. `eggserve-core` — compatibility forwarding/re-export changes.

Expected unchanged registry packages:

- `eggserve-primitives`;
- `eggserve-static`;
- `eggserve-h3`;
- `eggnet-tls`;
- `eggserve-bin` unless source changes are actually required by release
  metadata policy.

The likely next synchronized source patch after published core 0.2.2 is 0.2.3,
but do not assume it is still unused.

## Track B — Ensure core requires a server version that owns the forwarded feature

The published core package must never forward:

```toml
eggserve-server/tower
```

while permitting Cargo to resolve an older server release that does not expose
that feature.

Raise the core dependency requirement on `eggserve-server` to the first
published patch that contains Plan 276's `http-interop`/Tower feature
authority.

Keep the requirement compatible with the 0.2.x release policy; use the
repository's normal caret/exact convention rather than inventing a custom
range.

This requirement is why publication order matters:

```text
1. publish eggserve-server
2. wait for registry visibility
3. publish eggserve-core
```

Do not publish core first.

## Track C — Release metadata and changelog

Synchronize repository/package metadata using the existing release tooling.

Update `CHANGELOG.md` and release-facing docs to state:

- direct `eggserve-server` consumers can opt into `http-interop` / Tower;
- `eggserve-core` preserves the existing adapter source paths as
  compatibility re-exports;
- core remains the static/composition umbrella and therefore still carries
  static-serving dependencies;
- direct H1 + Tower consumers no longer need core;
- no static-serving, H1 transport, lifecycle, H2/H3, TLS, or Python behavior
  changed;
- the adapter remains experimental at its existing support tier.

Do not describe this as removal of static support from core.

## Track D — Exact release-candidate qualification

Run Plan 276's focused direct-server and compatibility gates on the exact
release candidate.

Then run all current release-relevant repository gates:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all
```

Also run:

```bash
cargo test --doc -p eggserve-server
cargo test --doc -p eggserve-core
cargo check -p eggserve-core --examples
```

where applicable to the current repository.

Push the release-readiness candidate and require hosted routine CI success on
that exact SHA before publication.

## Track E — Package dry-runs and staged local-registry proof

Before any crates.io publication, inspect the actual staged packages.

Required dry-runs, in dependency order:

```bash
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-core --locked --dry-run
```

The staged `eggserve-server` package must expose:

```toml
[features]
http-interop = [...]
tower = [...]
```

and contain all server-owned adapter source/tests required by packaging.

The staged `eggserve-core` package must forward those features and contain
only compatibility facade source for the migrated adapter modules.

Use the repository's layered local-registry/package verifier to prove the
published-package shape, not just workspace path dependencies.

A local-registry direct consumer must resolve:

```text
eggserve-server -> primitives
```

with the `tower` feature and no core/static/PHF ancestry.

If staged packaging fails that graph assertion, stop.

## Track F — Manual crates.io publication

Crates.io publication remains a maintainer action.

Expected order:

```bash
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-server --locked
# wait for sparse-index visibility

cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked
```

If only one package is actually source-changed after Plan 276, adjust to the
derived set and record why. Do not republish unchanged leaves.

If credentials are unavailable, leave the plan at
`release-ready / publication-pending` with the exact candidate SHA and
commands. Never claim registry closure from a local path build.

## Track G — Registry-only direct Axum consumer

After the new server release is visible, create a fresh consumer outside the
EggServe workspace with no path/git/patch dependencies.

Target manifest:

```toml
[dependencies]
eggserve-server = {
  version = "=<published-server-patch>",
  default-features = false,
  features = ["tower"]
}
axum = { version = "0.8", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
```

Add only minimal support crates required by the fixture.

Do not add `eggserve-core`.

Prefer no direct `eggserve-primitives` dependency if Plan 276's narrow
`RequestBodyPolicy` re-export makes it unnecessary. If direct primitives are
still required for a legitimate public type, record that explicitly; it is an
acceptable leaf dependency and does not invalidate the packaging goal.

The fixture must prove:

1. `eggserve_server::Server::builder()`;
2. pre-bound listener adoption;
3. `RuntimeConfig::disable_connection_total_timeout()`;
4. `eggserve_server::tower::TowerToEggserve::with_policy`;
5. an Axum 0.8 Router satisfies the adapter bounds;
6. finite request/response correctness;
7. incremental chunked request handling;
8. incremental streaming response before producer completion;
9. duplicate response headers;
10. middleware behavior;
11. disconnect cancellation;
12. cloneable `ServerControl` + passive typed
    `ServerCompletion::wait()` shutdown.

Then record:

```bash
cargo tree -e no-dev
cargo metadata --format-version 1
```

and prove dependency ancestry contains no:

- `eggserve-core`;
- `eggserve-static`;
- PHF-family edge from EggServe.

If another unrelated consumer dependency introduces PHF, show with
`cargo tree -i <crate>` that EggServe is not its ancestor.

## Track H — Registry-only core compatibility consumer

Separately create a tiny clean consumer for the existing compatibility path:

```toml
eggserve-core = {
  version = "=<published-core-patch>",
  default-features = false,
  features = ["tower"]
}
```

Prove the historical imports still compile and run:

```rust
eggserve_core::primitives::interop::HttpRequestBody
eggserve_core::server::TowerToEggserve
```

This consumer is expected to retain core's static/PHF dependency closure.
That is not a failure; it proves compatibility was preserved while the direct
path became lean.

## Track I — Quantify the downstream packaging change

Using the same toolchain/host/profile, compare the registry-only direct
consumer to the Plan-275/core-shaped consumer.

Record at least:

- lockfile package count;
- normal/no-dev dependency nodes;
- EggServe-owned package list;
- release executable bytes;
- `cargo tree -i eggserve-static`;
- `cargo tree -i phf` and PHF-family ancestry.

Use EggPool Plan 248's +9-package observation only as historical context. The
registry-only fixture is the qualification authority for this upstream
release.

A separate EggPool repository adoption can later measure the real application
delta. Do not modify EggPool from this plan.

## Track J — Closure evidence and roadmap reconciliation

Create a durable closure record, for example:

```text
release/plan-277-direct-tower-adapter-publication-closure.md
```

Record:

- Plan-276 implementation SHA;
- exact changed package set;
- selected patch version;
- focused/local/full qualification results;
- hosted CI run ID;
- package dry-run/local-registry results;
- publication timestamps;
- crates.io checksums;
- registry-only direct consumer manifest and resolved graph;
- registry-only core compatibility consumer result;
- dependency/package/binary comparison;
- final downstream disposition.

Update:

- `plans/ROADMAP.md`;
- Plan 276 status/evidence;
- Plan 277 status;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- release/process docs that state the current published adapter location.

Final disposition vocabulary:

- `COMPLETE — DIRECT STATIC-FREE ADAPTER PUBLISHED`;
- `PUBLICATION PENDING`;
- `CORRECTIVE REQUIRED`.

Do not call the direct path available to registry-only downstreams until the
published `eggserve-server` consumer proves it.

## Acceptance criteria

- [ ] Live registry state is checked immediately before version selection.
- [ ] The exact changed publish set is derived rather than assumed.
- [ ] The published server package exposes opt-in `http-interop`/Tower
      support.
- [ ] Core's server dependency floor guarantees the forwarded feature exists.
- [ ] Server is published before core when both change.
- [ ] Package dry-runs and staged local-registry tests pass.
- [ ] Hosted CI passes on the exact release candidate.
- [ ] A crates.io-only Axum consumer uses `eggserve-server --features tower`
      with no `eggserve-core` dependency.
- [ ] That direct consumer has no EggServe-owned path to
      `eggserve-static` or PHF.
- [ ] Streaming, duplicate-header, middleware, disconnect, and typed shutdown
      behavior pass from registry artifacts.
- [ ] A crates.io-only core consumer preserves the Plan-275 compatibility
      import paths.
- [ ] Package/dependency/binary deltas are recorded with ancestry evidence.
- [ ] No unchanged package is published for symmetry.
- [ ] No PyPI publication is required or falsely claimed.
- [ ] EggPool source is unchanged by this upstream release plan.
- [ ] Roadmap/current-authority docs are reconciled only after registry proof.

## Non-goals

- No removal of `eggserve-static` from `eggserve-core`.
- No new adapter crate.
- No EggPool implementation change.
- No framework/router behavior in EggServe.
- No H2/H3 tier change.
- No TLS change.
- No Python runtime change.
- No static-serving behavior change.
- No automatic crates.io publication from CI.

Plan 277 closes only when the published direct server package proves the
static-free Tower/Axum consumption path from a fresh registry-only consumer.
