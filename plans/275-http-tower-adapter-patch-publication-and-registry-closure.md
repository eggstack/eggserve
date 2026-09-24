# Plan 275 — HTTP/Tower adapter patch publication and registry-only downstream closure

## Purpose

Publish the Plan-274 HTTP/Tower ownership corrective as the smallest compatible
0.2.x Rust patch and prove the corrected adapter from a clean crates.io-only
Axum consumer before declaring downstream framework embedders unblocked.

This is the release/evidence closure for Plan 274. It must not reopen the
direct-server runtime work completed by Plans 270–273.

Planning baseline:

```text
091cddc release: close direct server downstream publication
```

Depends on:

- Plan 274 — HTTP/Tower request-body ownership corrective and Axum downstream
  qualification.

Current published baseline:

- `eggserve-core 0.2.1`;
- `eggserve-server 0.2.1`.

The expected next patch is `0.2.2`, but execution must query crates.io
immediately before changing metadata and choose the next unused compatible
`0.2.x` patch rather than assuming availability.

## Why a publication plan is required

Fixing `main` does not unblock downstream projects that intentionally consume
released crates and refuse git/path patches.

EggPool's previous adoption gate correctly stopped rather than carrying a
private HTTP-body/Tower workaround. The upstream work is complete only when a
clean external consumer can resolve the corrected `eggserve-core` feature
surface from crates.io and compose it with the already-published direct
`eggserve-server` runtime.

The registry proof must cover the real framework shape, not merely
`cargo check` a synthetic Tower service.

## Track A — Establish the exact changed publish set

Before version edits, derive the changed package set from the Plan-274 diff and
`cargo metadata`.

Expected result:

- `eggserve-core` changes and requires publication;
- `eggserve-server` should remain unchanged and should not be republished
  solely for numerical symmetry;
- `eggserve-primitives` should remain unchanged and should not be republished;
- static/H3/TLS/bin crates should remain unchanged.

The repository uses synchronized workspace/Python release metadata, so source
metadata may move to the next patch even when only one Rust crate requires
crates.io publication. Do not confuse synchronized source metadata with the
minimal changed registry publish set.

If implementation evidence shows another crate source actually changed in a
way required by the package graph, publish it in dependency order and record
why. Do not broaden the publish set preemptively.

## Track B — Patch-version and changelog metadata

Immediately before selecting the patch version:

1. query crates.io for the latest `eggserve-core` and related changed crates;
2. verify the intended version is unused;
3. synchronize repository version metadata using the existing release policy;
4. update `CHANGELOG.md` with the adapter correction and feature-gate
   qualification;
5. keep Python runtime behavior explicitly unchanged.

The patch notes should state that:

- `http-interop`/`tower` in the prior release could not compile because the
  compatibility crate attempted an external-trait-for-external-type impl after
  `RequestBody` moved to the primitives crate;
- the corrected patch introduces/uses a core-owned HTTP request-body adapter;
- the canonical `RequestBody` and direct H1 runtime contracts are unchanged;
- adapter/server APIs remain experimental;
- routine CI now covers the advertised adapter feature profiles.

Do not claim a published version until crates.io confirms it.

## Track C — Full source and package qualification

Run Plan 274's focused feature gates on the exact release candidate:

```sh
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features http-interop
cargo +1.89 check -p eggserve-core --all-targets --no-default-features --features tower
cargo clippy -p eggserve-core --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-core --no-default-features --features tower
```

Then run the current complete release-relevant repository gates:

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
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all
```

Run routine remote CI on the exact release-readiness candidate and record the
candidate SHA/run ID.

No wheel release or PyPI publication is required for this Rust adapter patch.

## Track D — Package-content and feature verification

Before publishing, inspect the staged `eggserve-core` package rather than
assuming workspace success implies registry-package success.

The packaged crate must contain the corrected interop/Tower sources and tests
needed by package verification, and its normalized manifest must expose:

```toml
[features]
http-interop = [...]
tower = [...]
```

with no path-only assumption required for those features to compile after
registry resolution.

Run at minimum:

```sh
cargo publish -p eggserve-core --locked --dry-run
```

plus the repository's layered local-registry/package verification.

If the package dry-run or staged local-registry consumer cannot compile
`--features tower`, stop. Do not publish.

## Track E — Manual crates.io publication

Crates.io publication remains a maintainer action.

For the expected core-only changed set:

```sh
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked
```

Wait for sparse-index visibility before starting the registry consumer proof.

If another changed crate is verified as required, publish in dependency order.
Do not republish unchanged crates merely to keep version numbers aligned.

Versions are immutable. If the artifact is wrong after publication, prepare a
new patch; never attempt to overwrite it.

If credentials/publication are unavailable during implementation, leave this
plan truthfully at `release-ready / publication-pending` and hand off the exact
candidate SHA and commands.

## Track F — Clean registry-only Axum consumer proof

After the corrected `eggserve-core` patch is visible on crates.io, create a
fresh temporary consumer outside the EggServe workspace.

It must contain no:

- path dependencies;
- git dependencies;
- copied EggServe source;
- `[patch.crates-io]` entries.

Use the corrected core patch exactly and the compatible published direct
server:

```toml
[dependencies]
eggserve-core = { version = "=<new-patch>", default-features = false, features = ["tower"] }
eggserve-server = { version = "0.2", default-features = false }
axum = { version = "0.8", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
```

Add only small support dependencies actually needed by the fixture.

The consumer must prove the public composition:

```text
pre-bound TcpListener
  -> eggserve-server::Server
  -> TowerToEggserve::with_policy(axum::Router, ...)
  -> Axum handler/middleware
  -> incremental Axum response body
```

Required registry-only checks:

1. `cargo tree` and `cargo metadata` resolve the new `eggserve-core` patch
   from crates.io and a compatible published `eggserve-server`;
2. the Axum Router satisfies the adapter type bounds with no local wrapper;
3. a normal request receives the expected status/body;
4. a chunked request is accepted through the streaming body path;
5. an Axum `Body::from_stream` response is observed incrementally before the
   producer completes;
6. duplicate response headers survive according to the documented interop
   semantics;
7. external shutdown uses direct `ServerControl` while
   `ServerCompletion::wait()` yields a typed clean result;
8. the process exits without a detached runtime task.

Prefer deterministic barriers/channels for the streaming assertion.

Record exact resolved crate versions and crates.io checksums.

## Track G — Downstream-facing evidence record

Create a durable closure record, for example:

```text
release/plan-275-http-tower-adapter-patch-publication-closure.md
```

Record:

- Plan-274 implementation candidate SHA;
- focused adapter feature gate results;
- routine CI run ID;
- package/local-registry verification;
- chosen patch version;
- publication timestamps;
- crates.io checksums;
- registry-only consumer manifest and resolved versions;
- the streaming/supervision smoke result;
- final disposition.

The downstream disposition is `UNBLOCKED` only if the registry-only Axum
consumer succeeds.

If source qualification is complete but publication or registry smoke is not,
use `PUBLICATION PENDING` rather than `UNBLOCKED`.

## Track H — Roadmap and documentation closure

After successful publication/proof:

- reconcile `plans/ROADMAP.md`;
- mark Plan 274 complete with its implementation SHA/evidence;
- mark Plan 275 complete with registry proof;
- update `docs/release-process.md` current Rust patch description;
- ensure `docs/http-interop.md` reflects the published wrapper API;
- update `AGENTS.md` and the development skill with the new published adapter
  baseline only if those current-authority summaries mention versions/status.

Do not rewrite historical Plan 200 or EggPool evidence. Those records explain
why the corrective exists.

## Acceptance criteria

- [x] The next unused compatible 0.2.x patch is selected from live crates.io
      state rather than guessed.
- [x] Plan 274 is implemented and all focused adapter feature gates pass.
- [x] Routine local/security/package qualification passes on the exact
      release candidate.
- [x] Routine CI passes on the exact release candidate SHA.
- [x] The actual changed Rust publish set is derived and recorded.
- [x] `eggserve-core` package dry-run succeeds with the corrected feature
      surface.
- [x] The corrected core patch is published and registry-resolvable before
      downstream unblock is claimed.
- [x] A fresh registry-only consumer resolves the corrected core patch with no
      git/path/patch override.
- [x] The registry consumer composes `eggserve-server`,
      `TowerToEggserve`, and Axum 0.8 using only public APIs.
- [x] Request/response streaming remains incremental in the registry consumer.
- [x] Direct control/completion supervision closes cleanly around the Axum
      adapter.
- [x] Exact versions/checksums and execution evidence are retained.
- [x] No unrelated crate is republished merely for version symmetry.
- [x] No PyPI publication is required or falsely claimed.
- [x] Roadmap/current-authority docs are reconciled only after the registry
      artifact is proven.

## Non-goals

- No EggPool code change in this repository.
- No direct-server runtime redesign.
- No static-serving change.
- No H2/H3 support-tier promotion.
- No new framework abstraction.
- No automatic crates.io publication from CI.
- No PyPI wheel publication requirement.
- No broad dependency addition beyond test-only Axum qualification.

Plan 275 closes when the corrected registry artifact and clean Axum consumer
proof both exist. With both now recorded, downstream framework embedders can
consume the generic adapter from crates.io without a private replacement.

## Execution status

**Complete — downstream unblocked (2026-09-24).** crates.io was queried
through Cargo's live registry index on 2026-09-24 immediately before version
selection: `eggserve-core 0.2.1` was latest, `eggserve-core 0.2.2` was absent,
and `eggserve-server 0.2.1` was published. Workspace and Python package source
metadata are synchronized to 0.2.2; Python runtime behavior is unchanged.

The Plan-274 implementation diff changes only `eggserve-core` production
source/manifest among registry crates. The other diffs are routine CI, docs,
tests, and synchronized release metadata. Therefore the derived registry
publish set is `eggserve-core` only; no unchanged server or primitives crate
will be republished. `scripts/verify-cargo-packages.sh --mode all` passed its
staged local-registry checks for all workspace packages, including the
corrected `eggserve-core` package. The candidate also passed focused Rust 1.89
interop/Tower checks, Tower Clippy/tests (1,109 passed, 2 ignored), standalone
interop tests, workspace tests/Clippy, H2/TLS and H3/TLS suites, topology,
conformance, format, Python metadata and locked crate checks, and the dual
lockfile supply-chain audit. The first `verify.sh fast` invocation stopped at
the excluded Python check because its separate lockfile still had version
0.2.1; that lockfile was synchronized and its required `--locked` check passed.

The clean-commit `cargo publish -p eggserve-core --locked --dry-run` passed on
candidate `5105a63d1d1569c4646c05c8d6fd96e83efe71ea`; it packaged and verified
`eggserve-core 0.2.2` and resolved the already-published
`eggserve-server 0.2.1`. Hosted CI run
[`35959464673`](https://github.com/eggstack/eggserve/actions/runs/35959464673)
passed on that exact SHA (Rust, Python, and supply-chain jobs). The published
artifact and fresh registry-only Axum consumer proof, including resolved
versions/checksums, are retained in
`release/plan-275-http-tower-adapter-patch-publication-closure.md`.
