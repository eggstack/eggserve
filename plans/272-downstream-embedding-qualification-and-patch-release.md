# Plan 272 — Downstream embedding qualification and 0.2.x Rust patch-release readiness

## Purpose

Qualify Plans 270–271 together as one coherent downstream embedding contract,
remove the remaining version-hardcoded package-verification debt, and prepare a
publishable 0.2.x Rust patch that actually unblocks external consumers.

This is the closure/release-readiness plan for the direct-server embedding
work. It does not add another runtime feature.

Depends on completed implementations of:

- Plan 270 — independent direct-server control/completion supervision;
- Plan 271 — explicit unlimited total connection lifetime.

## Why this plan is needed

Source changes on EggServe `main` do not unblock registry consumers.
The downstream use case that exposed Plans 270–271 intentionally consumes
published `eggserve-server = "0.2"` and must not depend on a git revision.

The repository also contains release-verification code that hardcodes
`0.2.0` in `scripts/verify-cargo-packages.sh`:

- generated staged workspace version;
- path-dependency rewrite patterns;
- staged crate filenames.

A 0.2.1 source version would make that package gate stale or incorrect unless
the script is generalized first.

## Scope

This plan has four responsibilities:

1. prove the two new direct embedding capabilities work together using only
   the public leaf crates;
2. generalize layered package verification so it derives the current version
   instead of embedding `0.2.0`;
3. prepare the synchronized repository metadata for the next compatible
   `0.2.x` patch release;
4. perform or hand off the manual crates.io publication required for registry
   consumers.

No Python runtime behavior changes are required.

## Track A — Public downstream compile/runtime fixture

Add one small test/example/fixture that depends only on the public direct
surface:

```text
eggserve-server
eggserve-primitives
tokio
```

It must demonstrate the real embedding pattern:

1. caller binds a Tokio `TcpListener` first;
2. runtime config explicitly disables total connection lifetime;
3. caller builds EggServe from the pre-bound listener;
4. caller starts one custom `Service`;
5. caller splits server control from completion;
6. caller supervises completion against an external shutdown future with
   `tokio::select!`;
7. external shutdown calls the independent control half;
8. completion returns the typed clean result;
9. repeated requests on one healthy keep-alive connection remain possible past
   a short comparison total-lifetime interval.

Do not import:

- `eggserve-core`;
- `eggserve-static`;
- Hyper directly;
- Tower/Axum;
- TLS/H2/H3 features.

This fixture is the direct proof that the leaf-crate contract is sufficient for
a small supervised daemon.

Keep it generic; do not name Gregg types or copy Gregg protocol behavior into
EggServe.

## Track B — Package-verification version authority

Refactor `scripts/verify-cargo-packages.sh` so the current workspace/package
version is derived once from Cargo metadata and used everywhere.

Remove hardcoded release-version assumptions from at least:

- generated temporary workspace `[workspace.package].version`;
- staged dependency rewrite expectations;
- generated crate filenames;
- local-registry package paths.

The script must continue to prove the complete layered package graph without
network publication.

Add a cheap self-check or shell/Python fixture proving the script does not
contain the current literal release version as behavioral authority.

Do not replace one hardcoded `0.2.0` with hardcoded `0.2.1`.

If another release/package script contains the same stale-version pattern,
correct it in this plan only when it is part of the Rust patch preflight.

## Track C — Patch-version metadata

After Plans 270–271 and Track B are green, choose the next available compatible
0.2.x patch version (expected `0.2.1` if still unused at execution time).

Before editing metadata, query crates.io/current repository tags so the version
is not guessed.

Follow the repository's synchronized metadata policy:

- workspace Rust version;
- excluded Python crate/version metadata required by
  `scripts/check-python-release-metadata.py`;
- `pyproject.toml`/other synchronized metadata;
- internal version references that are true release metadata;
- changelog/release notes.

This metadata synchronization does **not** require a PyPI publication in this
plan. The runtime changes are Rust direct-server capabilities; Python wheel
publication remains a separate maintainer decision.

Do not change dependency requirements from `0.2.0` to exact `0.2.1` merely
for symmetry when ordinary `^0.2.0` compatibility already permits the patch.
Only update dependency declarations when repository package validation or a
real API dependency requires it.

## Track D — Source/API compatibility qualification

Prove that the patch remains additive.

At minimum retain/compile existing 0.2.0 usage for:

- direct `Server::builder`;
- pre-bound listener adoption;
- `ServerHandle::shutdown`;
- legacy `ServerHandle::wait(self) -> ()`;
- direct `RuntimeConfig` public field access/struct construction fixtures;
- compatibility-core server construction.

Add compile tests for the new:

- control/completion split;
- typed completion result;
- unlimited total-lifetime builder convenience.

Do not mark the patch release-ready if existing 0.2.0 source fixtures require
changes.

## Track E — Full local/repository qualification

Run the current complete gates on the final source candidate:

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
ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all
```

Use the exact commands current repository docs require if they change before
execution.

Run routine remote CI on the exact release-readiness candidate and record the
run ID/SHA.

No new CI job or benchmark framework is required.

## Track F — Rust crates.io publish set

The downstream blocker is resolved only by a published Rust crate.

Determine the minimal changed publish set from the final diff and dependency
graph. At the planning baseline this is expected to include:

1. `eggserve-server` — mandatory, because Plans 270–271 change its public
   runtime contract;
2. `eggserve-core` — publish when Plan 271 changes compatibility runtime
   behavior/docs in that crate.

`eggserve-primitives` is not expected to require source changes for this
work. Do not republish unrelated leaf crates solely for numerical symmetry if
the repository's crates.io policy permits the changed crates to depend on the
already-published compatible 0.2.0 leaves.

Before publication, use `cargo metadata` and package dry-runs to verify the
actual registry dependency graph. If synchronized publication is required by
current policy/tooling, follow the canonical layered order rather than forcing
the smaller expected set.

Crates.io publication remains a manual maintainer action. The implementation
agent may prepare and dry-run it but must not invent credentials or claim a
release exists before crates.io confirms it.

Expected manual order when only the two changed crates need publication:

```sh
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-server --locked

# wait for crates.io index visibility

cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked
```

Adjust only from verified package dependencies/current release policy.

## Track G — Published-consumer smoke

After crates.io confirms the new `eggserve-server` patch:

Create a clean temporary consumer with no workspace path patches and resolve:

```toml
eggserve-server = { version = "0.2", default-features = false }
eggserve-primitives = { version = "0.2", default-features = false }
```

Prove the registry-resolved source exposes:

- pre-bound listener adoption;
- the Plan-270 supervisory lifecycle split;
- the Plan-271 unlimited total-lifetime control;
- the custom direct `Service` path.

Run the minimal supervised loopback smoke from Track A against the registry
artifact.

Record the exact resolved crate versions and checksums.

If publication is not performed during the implementation handoff, Plan 272
must remain **release-ready / publication pending**, not complete. That state is
truthful and still gives the maintainer an exact final action.

## Documentation and roadmap closure

Update after implementation:

- `plans/ROADMAP.md`;
- `docs/public-api-boundary.md`;
- `docs/release-process.md` if the Rust publish graph/version-authority
  cleanup changes it;
- `docs/migration-guide.md`;
- `CHANGELOG.md`;
- relevant EggServe development/release skill docs.

Record that:

- 0.2.0 remains the historical initial direct-server release;
- the new patch is additive;
- default connection lifetime remains 60 seconds;
- critical supervisors should use the new typed control/completion path;
- unlimited total lifetime is opt-in only;
- Python defaults and support tiers are unchanged. The existing
  `connection_total_timeout_secs=0` setting now uses the shared opt-out
  semantics; finite defaults and the Python API shape are unchanged.

## Acceptance criteria

- [ ] A leaf-crate-only downstream fixture proves pre-bound listener +
      independent supervision + explicit unlimited total lifetime together.
- [ ] The fixture uses no `eggserve-core`, Axum/Tower, direct Hyper, TLS,
      H2, H3, or static serving.
- [ ] Existing 0.2.0 direct API compile fixtures remain source-compatible.
- [ ] `scripts/verify-cargo-packages.sh` derives the release version instead
      of hardcoding `0.2.0`/the new patch.
- [ ] Package verification passes for the complete staged layered graph at the
      new version.
- [ ] Repository/Python metadata is synchronized to the next verified 0.2.x
      patch version.
- [ ] Full local/security/package gates pass.
- [ ] Routine CI passes on the exact release-readiness candidate SHA.
- [ ] The required Rust publish set is derived from the real package graph and
      dry-runs cleanly.
- [ ] `eggserve-server` is published to crates.io before downstream unblock is
      claimed.
- [ ] A clean registry-only consumer resolves the new patch and passes the
      supervised loopback smoke.
- [ ] If manual publication has not occurred, status remains explicitly
      publication-pending rather than complete.
- [ ] No protocol-tier change or unrelated release work is pulled into this
      patch. The Python wheel suite is rerun because the existing timeout
      parameter also needed to preserve independent handler/body budgets when
      total lifetime is disabled.

## Non-goals

- No PyPI publication requirement.
- No new wheel target or wheel-pipeline redesign.
- No GitHub Release/tag requirement.
- No new protocol capability.
- No default timeout change.
- No downstream application code in this repository.
- No automatic crates.io publication from push/CI.

Plan 272 is complete only when the registry artifact needed by downstream
consumers actually exists and the registry-only smoke passes. Until then,
Plans 270–271 may be implementation-complete while the downstream unblock
remains publication-pending.

## Implementation status

Tracks A–F are implementation-complete on the 0.2.1 release candidate:
the leaf-only fixture is present, package verification derives its version
from Cargo metadata, synchronized Rust/Python package metadata is 0.2.1,
existing 0.2.0 dependency constraints remain compatible, and local package
dry-runs qualify the layered graph. The changed publish set is
`eggserve-server` and `eggserve-core`. Track G remains pending because the
crates.io publish is a manual maintainer action; the candidate is
release-ready/publication-pending and no registry consumer unblock is claimed.

## Closure execution handoff

Plan 273 (`plans/273-direct-server-downstream-publication-and-evidence-closure.md`)
is the authoritative execution/evidence follow-up for the remaining Track G
work. It preserves this plan's implementation and qualification history,
performs the crates.io publication/registry-only smoke, creates the missing
`release/plan-272-downstream-embedding-qualification-closure.md`, and closes
the program only if the published consumer proof succeeds.

### Plan 273 closure

Plan 273 completed the manual publication and registry proof on 2026-09-23.
`eggserve-server 0.2.1` and `eggserve-core 0.2.1` are published; the fresh
registry-only supervised consumer resolves the 0.2.1 server and exits cleanly
with `ShutdownResult::Clean`. This records completion without rewriting the
publication-pending state that was true at this plan's implementation handoff.
Full provenance is in
`release/plan-272-downstream-embedding-qualification-closure.md`.
