# Plan 273 — Direct-server downstream publication and evidence closure

## Purpose

Finish the Plans 270–272 downstream-embedding program without reopening the
runtime implementation.

The 0.2.1 source candidate is already implemented and qualified on
`466cf6301f20c7202f696c495e6eb8d5e74664be`. The remaining work is release
execution and durable evidence:

1. publish the required Rust crates to crates.io;
2. prove a clean registry-only consumer resolves the new direct-server API and
   runs the supervised keep-alive fixture successfully;
3. create the missing Plan-272 closure evidence record already referenced by
   Plan 271;
4. reconcile Plans 270–272 and the roadmap only after the registry proof exists.

This is an execution/evidence closure plan, analogous to Plan 269 after its
implementation campaign. It does not authorize additional runtime behavior.

## Planning baseline

```text
466cf630 feat: qualify supervised direct server embedding
```

At this baseline:

- Plan 270 is implemented;
- Plan 271 is implemented;
- Plan 272 Tracks A–F are implemented;
- workspace/package metadata is synchronized at 0.2.1;
- `scripts/verify-cargo-packages.sh` derives version authority instead of
  hardcoding 0.2.0;
- routine CI run `35808907965` is green for Rust, Python, and supply-chain;
- the verified changed Rust publication set is
  `eggserve-server` + `eggserve-core`;
- Plan 272 is correctly marked release-ready/publication-pending;
- `release/plan-272-downstream-embedding-qualification-closure.md` is
  referenced by Plan 271 but does not yet exist.

## Constraints

- No production runtime change unless release execution exposes a concrete
  packaging defect that makes the already-qualified 0.2.1 source
  unpublishable.
- No new public API.
- No timeout-default change.
- No protocol-tier change.
- No H2/H3/TLS/static feature expansion.
- No PyPI publication requirement.
- No new wheel target or wheel-workflow redesign.
- No automatic crates.io publication from push/CI.
- Do not republish unrelated crates solely to keep every workspace package on
  the same published patch number.
- Do not claim downstream unblock until a clean registry-only consumer proves
  it.

If publication exposes a source/package defect requiring code changes, stop,
record the exact failure, and open a narrow corrective plan rather than
silently editing the release candidate under this evidence plan.

## Track A — Reconfirm immutable release candidate

Before publication, verify that the intended candidate is still the current
qualified source.

Record:

```text
candidate SHA: 466cf6301f20c7202f696c495e6eb8d5e74664be
routine CI:    35808907965
version:       0.2.1
```

If `main` has advanced only through documentation/planning commits after the
candidate, distinguish:

- implementation candidate SHA;
- final metadata/planning SHA.

Do not imply that a later metadata-only SHA changed the runtime.

Re-run only the cheap release-sensitive gates needed to ensure no metadata
drift has occurred:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-core --locked --dry-run
```

If current repository instructions require a different clean-tree invocation
for the package verifier, follow the current documented command and record it.

## Track B — Verify crates.io state before publishing

Do not assume 0.2.1 is still unpublished.

Immediately before publication, query crates.io for:

- `eggserve-server`;
- `eggserve-core`.

Record their latest versions and timestamps.

Possible outcomes:

### Neither 0.2.1 exists

Proceed with the planned publication order.

### Both 0.2.1 versions already exist

Do not attempt to publish again. Verify their source/checksum identity through
the registry-only consumer in Track D and continue to evidence closure.

### Only one exists

Treat the existing crate as immutable. Verify it corresponds to the intended
0.2.1 package, then publish only the missing required crate if its dependency
graph is satisfiable.

### A conflicting 0.2.1 exists

Stop. Do not attempt to overwrite an immutable crates.io version. Open a
separate corrective/version-bump plan (normally 0.2.2) with the exact
difference and reason.

## Track C — Publish the minimal Rust crate set

The expected publication set is:

1. `eggserve-server 0.2.1`;
2. `eggserve-core 0.2.1`.

The unchanged leaf crates remain valid at their already-published compatible
0.2.0 versions unless current package metadata proves otherwise.

Use the canonical manual maintainer flow.

Expected order:

```sh
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-server --locked
```

Wait until the registry/index can resolve the exact new server version before
publishing core:

```sh
cargo info eggserve-server@0.2.1
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked
cargo info eggserve-core@0.2.1
```

The implementation agent must not invent credentials, bypass owner policy, or
claim success from a local package alone. If credentials/manual maintainer
action are unavailable, stop at **publication pending** with the dry-run
evidence intact.

Record:

- exact published versions;
- publication timestamps where available;
- crates.io package links/identifiers;
- package checksums if exposed by Cargo/registry metadata;
- the exact source candidate from which the packages were produced.

## Track D — Clean registry-only consumer proof

After `eggserve-server 0.2.1` is visible in the registry, create a temporary
consumer outside the EggServe workspace.

Requirements:

- no path dependencies;
- no `[patch.crates-io]`;
- no git dependencies;
- no copied EggServe source;
- default features disabled for the two EggServe leaf crates;
- a fresh Cargo lockfile generated from the registry.

Minimal dependency shape:

```toml
[dependencies]
eggserve-server = { version = "0.2", default-features = false }
eggserve-primitives = { version = "0.2", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "io-util", "sync", "time"] }
```

Adapt the already-qualified
`crates/eggserve-server/tests/downstream_embedding.rs` behavior rather than
inventing a second semantic test.

The registry-only smoke must prove all of the following in one execution:

1. caller pre-binds a Tokio `TcpListener`;
2. `ServerBuilder::from_listener` adopts it;
3. `RuntimeConfigBuilder::disable_connection_total_timeout()` is available;
4. one healthy keep-alive TCP connection serves a second request after an
   elapsed interval longer than a short comparison total-lifetime value;
5. `ServerHandle::into_parts()` returns independent control/completion
   capabilities;
6. completion can be selected against an external shutdown future;
7. the shutdown branch retains and uses `ServerControl`;
8. `ServerCompletion::wait(&mut self)` returns
   `ShutdownResult::Clean`;
9. the program exits cleanly without detached runtime tasks.

Capture:

```sh
cargo tree
cargo metadata --format-version 1
cargo run
```

Record the resolved versions from the generated `Cargo.lock`. The critical
assertion is that `eggserve-server` resolves to the newly published patch,
not the old 0.2.0 crate.

Retain the registry checksum from `Cargo.lock` or equivalent Cargo metadata
for the new server/core artifacts when available.

## Track E — Create the missing durable closure record

Create exactly:

```text
release/plan-272-downstream-embedding-qualification-closure.md
```

This path is already referenced by Plan 271 and therefore should become the
canonical closure record rather than changing the historical reference to a
new filename.

The record must include:

### Implementation provenance

- Plan 270/271 planning baseline;
- implementation candidate
  `466cf6301f20c7202f696c495e6eb8d5e74664be`;
- concise description of the control/completion API;
- concise description of the zero/unlimited total-lifetime semantics;
- source-compatibility statement for the retained 0.2.0 direct APIs.

### Qualification provenance

- local/full verification summary already performed for Plan 272;
- remote routine CI run `35808907965`;
- Rust/Python/supply-chain conclusions;
- package dry-run result;
- version-derived package-verification result;
- any later metadata-only SHA clearly distinguished from the implementation
  candidate.

### Publication provenance

- crates.io versions and timestamps;
- publication order;
- checksums when available;
- whether only server/core were published or the verified package graph forced
  a broader set.

### Registry-consumer proof

- exact consumer manifest;
- resolved crate versions;
- relevant Cargo.lock checksums;
- command/results for `cargo tree`, `cargo metadata`, and the supervised
  loopback execution;
- explicit statement that no path/git/patch override was present.

### Downstream disposition

Conclude one of:

- **UNBLOCKED:** the registry-only consumer proves the published API needed by
  downstream direct embedders;
- **PUBLICATION PENDING:** implementation/qualification is complete but the
  manual publish has not occurred;
- **BLOCKED:** publication or registry-only smoke exposed a concrete defect.

Do not use UNBLOCKED merely because `main` compiles.

## Track F — Planning/documentation reconciliation

Only after the registry-only smoke succeeds:

- update Plan 270's implementation status to **complete through Plan 273
  registry qualification**;
- update Plan 271's implementation status so its existing closure-record
  reference now resolves to the actual file;
- append a Plan-273 closure note to Plan 272 rather than erasing its
  publication-pending history;
- update `plans/ROADMAP.md` to mark Plans 270–273 closed/complete;
- update `CHANGELOG.md` if it still describes 0.2.1 as unreleased or
  publication-pending;
- update `docs/release-process.md` only if actual publication exposed a
  difference from the documented Rust release procedure.

Do not rewrite Plan 272 as though publication was already complete at its
implementation commit.

If publication remains pending, leave all current pending language truthful and
record Plan 273 as awaiting maintainer publication.

## Verification

No new runtime qualification campaign is required. The implementation
candidate already passed routine CI.

Before final closure, run the minimum repository checks affected by the
evidence/docs update:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
```

If any production/package file changes after `466cf630`, the reduced
evidence-only matrix is no longer sufficient. Re-run the full Plan-272
qualification and routine CI on the corrected source candidate.

## Acceptance criteria

- [x] crates.io state is checked immediately before any publish attempt.
- [x] No immutable existing 0.2.1 artifact is overwritten or assumed.
- [x] `eggserve-server 0.2.1` is published and registry-resolvable.
- [x] `eggserve-core 0.2.1` is published when still required by the verified
      changed package set.
- [x] Publication uses the verified 0.2.1 source candidate/package contents.
- [x] A fresh external consumer uses only crates.io dependencies with no
      path/git/patch override.
- [x] The consumer resolves the new `eggserve-server` patch.
- [x] The consumer proves pre-bound listener adoption.
- [x] The consumer proves the control/completion supervisory split.
- [x] The consumer proves explicit unlimited total connection lifetime and
      keep-alive reuse past a comparison lifetime.
- [x] The consumer shuts down cleanly with a typed completion result.
- [x] Resolved versions/checksums and execution commands are retained.
- [x] `release/plan-272-downstream-embedding-qualification-closure.md`
      exists and contains implementation, CI, publication, and registry-smoke
      provenance.
- [x] Plan 271's existing evidence reference resolves to that tracked file.
- [x] Roadmap and Plans 270–272 are reconciled without rewriting historical
      publication-pending state.
- [x] Downstream unblock is claimed only after the registry-only smoke passes.

## Non-goals

- No new EggServe feature.
- No runtime optimization.
- No API redesign.
- No PyPI release requirement.
- No GitHub Release/tag requirement.
- No unrelated workspace-wide version churn.
- No additional protocol qualification.
- No downstream Gregg code change in this repository.

## Closure rule

Plan 273 closes only when the published registry artifact and registry-only
consumer proof both exist.

If manual crates.io publication is unavailable, the correct result is not a
workaround: keep Plan 273 open as publication-pending and hand the exact publish
commands/evidence to the maintainer.

## Execution status

**Complete — downstream unblocked (2026-09-23).** Both packages were published,
the registry-only smoke passed, and durable evidence is recorded in
`release/plan-272-downstream-embedding-qualification-closure.md`.
