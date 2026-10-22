# Direct H1 Runtime Milestone 305 — Corrective publication of the post-0.4.0 fix batch

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-11--corrective-040x-publication`

Original milestone and its closure (corrected by this plan):

- `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md`
- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` (conditionally closed)

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`
- `plans/003-planning-process.md#25-closure-records`

Applicable prior publication precedent:

- `release/plan-286-embedding-contract-publication-closure.md`
- `release/plan-291-direct-h1-boundary-ownership-publication-closure.md`
- `docs/release-process.md`
- `docs/migration-guide.md`

Primary class: invariant

## 1. Objective

Publish a new release line that carries the corrective batch committed
after the `v0.4.0` tag — `v0.4.0..465b80c` — so that the published
artifacts stop shipping the B1–B89 defects and the CI fix that the
currently-published 0.4.0 line still contains.

The corrective input is already implemented, reviewed, and green in the
repository. This milestone is a **version + publication + disclosure**
milestone, not a re-implementation milestone.

## 2. Why this milestone is ready

- The corrective code is already landed and in the tree at the baseline
  (five production-fix commits, 53 changed files, 1,646 insertions,
  522 deletions under `crates/`).
- Milestone 304 establishes the disclosure that made this work necessary
  and hands over the exact corrective input range.
- Publication precedent (286/291) and the local layered package gate are
  established and reusable.
- Hard dependencies (294–297, 298, 299, 304) are closed.

No new implementation work may be added to this milestone beyond what is
strictly required to publish what is already in the tree. If a defect in
the batch is discovered during qualification, it belongs in a separate
corrective plan, not here.

## 3. Current implementation evidence

At the baseline:

- The published 0.4.0 line (`v0.4.0` = `d537d4d`) predates every
  production-fix commit. See
  `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` for
  the full defect enumeration.
- The tree at `465b80c` contains the fixes. Representative defect classes
  already corrected in-tree:
  - conditional-request correctness (`If-Match *`, `If-Modified-Since`,
    first-wins vs comma-join header combining);
  - framing correctness (non-numeric `Content-Length` → 400, digit-only
    parse, `Http1Upgrade` requires a protocol, `Expect` → 417, `499` → 500
      collapse);
  - range/conditional interaction (unsatisfiable `Range` with failed
      `If-Range`);
  - confinement (child resolution inherits the confined backslash policy;
      fd-relative resolution re-applies platform component checks);
  - failure mapping (`PrematureEof`/`LengthMismatch` →
      `IncompleteError`; buffer-export failure fails the stream);
  - robustness (`version_to_http` and `from_state` fail closed instead of
      panicking or silently downgrading; listener/address pair cannot be
      mispaired);
  - shutdown and admission (drain bounded by `graceful_shutdown_timeout`;
      `try_acquire` + 503 parity; queued Python callback permits restored
      behind the documented queue-behind-permit contract).
- `CHANGELOG.md` has no `0.4.0` entry and no entry for this batch.
- The tree's manifest versions are the 0.4.0-line versions
  (`primitives 0.2.2`, `server`/`static`/`h3`/`core 0.4.0`, `bin 0.2.2`,
  wheel `0.2.4`), which are already published and therefore cannot be
  reused.

## 4. Invariants that must not regress

- Published crate versions are immutable. The corrective release MUST use
  new version numbers; the published 0.4.0 artifacts are never re-uploaded,
  altered, or yanked.
- The `RUSTSEC-2026-0285` `rustls` `0.23.45` caret floor stays in every
  constraining manifest, including the excluded Python crate.
- `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for
  both lockfiles; `aws-lc-rs` stays dev-only and the production tree stays
  ring-only.
- No behavior change is introduced by this milestone. The behavior change
  is already in the tree and is only being published. If a behavior change
  is required to publish, stop.
- Safe defaults are unchanged: loopback bind, no symlinks, no dotfiles, no
  directory listing unless explicitly opted in.
- No serving outside the configured root. Descriptor/handle-relative
  confinement is unchanged.
- No `println!`/`eprintln!` in library code.
- Crate topology ownership is unchanged; the topology gate passes before and
  after the version bumps.
- The manual publication workflow stays manual: no push, tag, or merge
  publishes anything.
- H2/H3 tiers and experimental classifications are unchanged.

## 5. Scope

### In scope

- The semver decision for the corrective line, recorded with its rationale.
- Version and requirement bumps for the crates that carry corrected
  behavior, plus the wheel, plus both lockfiles.
- The full local gate suite, exact-SHA hosted CI, serial crates.io
  publication in dependency order with index-visibility waits, registry-only
  consumer proof, tag, GitHub Release, and the wheel path.
- `CHANGELOG.md` entries for the 0.4.0 line and for the corrective line.
- `docs/migration-guide.md` entries for every published behavior change
  that a consumer can observe.
- The Plan 305 closure record and the registry/roadmap close.

### Explicitly out of scope

- Any production code change. If the batch needs a further fix, that is a
  separate corrective plan under `plans/implementation/direct-h1-runtime/`.
- Retouching the plans in this corrective series (306–309, 311–315). They
  are separate boundaries with separate closure records.
- The doc/skill/architecture wording sweep (Plan 314).
- Fuzz-harness repair (Plan 310), Python listing consolidation (Plan 311),
  and the tooling milestones (312, 313, 315).
- Promoting H2 or H3 out of the experimental tier.
- Yanking or editing the published 0.4.0 artifacts.
- A mass archive move of legacy plans.

## 6. Required production changes

### Crates and ownership

Version and requirement metadata only. The exact version numbers are a
decision of Work package A; the constraints are:

- No version number already present on the live index may be reused.
- The `eggserve-server` minor/patch choice must follow Work package A's
  semver decision and must be consistent across `eggserve-static`,
  `eggserve-h3`, and `eggserve-core`, which follow the server version
  (286 precedent).
- `eggserve-primitives` takes a patch bump if its public API is unchanged
  by the batch; if the batch changed a public primitive API, it takes a
  minor bump and the dependency is recorded.
- `eggserve-bin` follows the server requirement floor (286 precedent).
- The excluded Python crate and the wheel take a new version; the
  three-way version agreement enforced by
  `scripts/check-python-release-metadata.py` (workspace `Cargo.toml`,
  Python crate `Cargo.toml`, `pyproject.toml`) must hold.
- `eggnet-tls` is not republished unless the batch changed it. At the
  baseline it has not changed since 2026-09-18 and its published 0.2.0
  content matches the local tree, so no republication is required; the
  workspace-inherited `0.2.4` version observation is recorded as a known
  limitation, consistent with Plan 300 §5.

### Config and policy

None. No configuration default changes.

### Protocol and compatibility

The batch changes observable behavior in several places. Each change must
be reflected in `docs/migration-guide.md`. At minimum, the implementing
agent must classify and document:

- `If-Match: *` without an ETag (RFC 9110 §13.1.1);
- header combining semantics (first-wins vs comma-join per header);
- `Content-Length` parse strictness and the resulting 400;
- `Expect` handling and the 417 outcome;
- `Range` + `If-Range` interaction;
- the `499` → `500` collapse on the `ServiceError` wire path;
- `HEAD` error responses carrying the `GET` length;
- `version_to_http` / `from_state` fail-closed behavior;
- the Python queued-callback permit contract.

The published 0.4.0 line keeps the old behavior; the corrective line
changes it. Downstream consumers must be able to tell the two apart from
the changelog and migration guide.

### Runtime and concurrency

None. The runtime behavior already lives in the tree.

### Frontend or operator surface

The wheel's corrected behavior (Python callback permit contract, error
mapping, conditional/range lookups) is published with the corrected Rust
crates. The Python `HTTPStatus`-shaped surface is unchanged in shape.

### Security and confinement

Supply-chain gates for both lockfiles run before and at publication. The
confinement-relevant corrections (B35 backslash inheritance, B36
platform component re-check, B39 error mapping, B48 buffer-export
fail-closed, B58 listener pairing) are named in the changelog so operators
can confirm the deployed version.

### Documentation and static guards

- `CHANGELOG.md`: a `0.4.0` entry (describing what was published on
  2026-09-25, including that it predates the corrective batch) and an
  entry for the corrective line.
- `docs/migration-guide.md`: the observable behavior changes.
- `docs/release-process.md`: the new current leaf line.
- `README.md`: dependency snippets must resolve against the corrective
  line.
- Topology, conformance-matrix, and Python release-metadata gates re-run
  after the bumps.

## 7. Ordered work packages

### Work package A — Semver decision

Decide, and record in the closure record, the version for each crate that
carries corrected behavior. The decision must weigh:

- the presence of observable behavior changes (not only internal fixes) —
  if a consumer's observable output changes, a patch bump understates the
  change;
- the 286/291 precedent of minor bumps when the server requirement moves
  across a `0.x` minor boundary;
- the constraint that no already-published version is reused.

Output: a version table equivalent to Plan 300 §6, plus a rationale
paragraph per crate.

Acceptance evidence: the version table is recorded before any manifest is
edited, and every chosen version is absent from the live index.

### Work package B — Version bumps, lockfiles, and docs

Apply the Work package A table. Refresh both lockfiles. Update
`docs/release-process.md`, `README.md`, `CHANGELOG.md`, and
`docs/migration-guide.md`.

Verify `git diff --stat` shows only manifests, lockfiles, and docs — no
source file changes.

Acceptance evidence: `git diff --stat` contains no `crates/**/src/**`
entries.

### Work package C — Local gates

Run the full §11 gate list: format, topology, conformance matrix, Python
release metadata, supply chain for both lockfiles, layered package dry-run
for every published crate, workspace tests, and the feature lanes
(`http2,tls`, `http3,tls`, `tower`, `http-interop`).

Record every outcome. Do not proceed on red.

Acceptance evidence: a recorded command/outcome table; no red gate is
carried into publication.

### Work package D — Exact-SHA hosted CI proof

Push the bump commit; require exact-SHA hosted CI success for the rust,
supply-chain, and python jobs before any upload (226/272/275/286/291
precedent).

Acceptance evidence: the run ID and conclusion, pinned to the bump commit
SHA.

### Work package E — crates.io publication

Publish serially in dependency order, confirming index visibility before
each dependent. Record UTC timestamps and SHA-256 checksums per crate.

On any failure: stop. Never re-upload a published version; cut a new one.

Acceptance evidence: a per-crate timestamp/checksum table.

### Work package F — Registry-only consumer proof

Fresh `/tmp` manifests pinned to the exact published versions (direct H1,
direct H1 + TLS, Tower/Axum, core compatibility), locked builds, focused
tests, and `cargo tree -e no-dev` ancestry proving the direct graph stays
free of `eggserve-core`/`eggserve-static`/`phf`.

Acceptance evidence: manifests, command output, and ancestry output.

### Work package G — Tag and GitHub Release

Tag the bump commit (`v<new-line>`) and create the GitHub Release with
notes that name the artifact set, checksums, the wheel status, and — in a
prominent "Upgrading from 0.4.0" section — the corrected defect classes.

Acceptance evidence: tag SHA and release URL.

### Work package H — Wheel publication

Dispatch `release.yml` with `publish_target=testpypi`, qualify, then
`publish_target=pypi` (the `pypi` environment approval is a maintainer
GitHub action and is outside agent hands — report the pending-approval
state rather than waiting silently).

Acceptance evidence: both run IDs, the aggregate wheel manifest, and the
post-publish smoke result.

### Work package I — Changelog and migration-guide finalization

Finalize the `CHANGELOG.md` entries so that a reader can tell exactly what
the 0.4.0 line contains, what the corrective line contains, and which
behavior changes require consumer attention.

Acceptance evidence: the changelog diff is reviewed against the Work package
A rationale and the Work package C gate results.

### Work package J — Closure

Write `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`
with the §15 evidence; close roadmap Milestone 11 and update the registry.

## 8. Failure, cancellation, restart, and contention semantics

- A published version is immutable: a partially published line is remediated
  with new versions, never by re-uploading.
- Dependents publish only after the prerequisite is index-visible; a
  visibility timeout stops the sequence and no partial-graph claim is made.
- A rejected wheel run does not affect the crates.io artifacts; recovery
  uses a new wheel version when contents must change.
- The release workflow concurrency group serializes dispatches; do not
  dispatch twice.

## 9. Compatibility and migration

- `0.4.0`-line consumers stay on the published artifacts; nothing is
  yanked.
- The corrective line is a **separate** version line, so the choice of
  minor vs patch determines whether consumers must change their
  requirement. The `docs/migration-guide.md` entries must make the
  observable behavior changes explicit either way.
- Consumers who pinned `eggserve-core = "0.4"` will resolve the corrective
  release automatically **only if** it is published as `0.4.x`. If
  Work package A chooses a new minor, the migration guide must state that
  explicitly so consumers know to widen the requirement.

## 10. Required tests

- `cargo publish -p <each> --locked --dry-run` for every published crate.
- `bash scripts/verify-cargo-packages.sh --mode all`.
- Registry-only consumers: direct H1 (+TLS), Tower/Axum, core compatibility —
  build plus focused tests plus `cargo tree -e no-dev` ancestry.
- Exact-SHA hosted CI green on the bump commit.
- Wheel matrix qualification via the dispatched workflow, not local
  emulation.
- The existing workspace test suite is the regression evidence for the
  batch itself; no new behavior tests are added by this milestone, because
  the batch's own commits carry them.

### Migration and compatibility tests

- A consumer pinned to the previous line still resolves and builds.
- The corrected behaviors are observable in the published artifact via the
  registry-only consumers (for example the `If-Match: *` behavior and the
  non-numeric `Content-Length` → 400 behavior).

## 11. Required verification commands

```bash
cargo fmt --all -- --check
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
cargo publish -p eggserve-primitives --locked --dry-run
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-static --locked --dry-run
cargo publish -p eggserve-h3 --locked --dry-run
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-bin --locked --dry-run
cargo test --workspace
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
```

## 12. Documentation updates

- `CHANGELOG.md` — the 0.4.0 entry and the corrective entry.
- `docs/migration-guide.md` — observable behavior changes.
- `docs/release-process.md` — the current leaf line.
- `README.md` — dependency snippets that resolve against the corrective
  line.
- `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`
  (new), plus registry and roadmap updates.

## 13. Acceptance criteria

- The published corrective line carries the full `v0.4.0..465b80c` batch;
  the published artifacts are demonstrably newer than the 0.4.0 artifacts.
- Every version in the Work package A table was absent from the live index
  before publication, and none was reused.
- All §11 gates are green locally and exact-SHA hosted CI is green on the
  bump commit.
- crates.io shows every published version with a recorded checksum, and each
  dependent was published only after prerequisite index visibility.
- Registry-only consumers prove direct-graph purity and core resolution
  against the corrective versions.
- The tag and GitHub Release exist and name the corrected defect classes.
- The wheel is qualified on TestPyPI and published to PyPI (the `pypi`
  approval is explicitly handed off).
- `CHANGELOG.md` and `docs/migration-guide.md` let a consumer identify the
  0.4.0 line, the corrective line, and every observable behavior change.
- The closure record exists; roadmap and registry agree; no stale control
  state remains for this milestone.

## 14. Stop conditions

Stop and report rather than improvise if:

- any §11 gate is red — fix the cause, do not publish around it;
- exact-SHA hosted CI on the bump commit is not green;
- a chosen version already exists on the live index;
- index visibility for a prerequisite never arrives;
- an upload fails after others succeeded — remediate with new versions;
- the `pypi` environment approval is unavailable — wheels stay at TestPyPI
  and the state is reported;
- the diff shows source changes beyond manifests, lockfiles, and docs;
- qualification reveals a defect in the corrective batch — that defect
  belongs in a separate corrective plan, not in this publication.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`
containing:

- the Work package A semver rationale and the version table;
- the bump commit SHA and exact-SHA hosted CI run ID/conclusion;
- per-crate UTC timestamps and crates.io SHA-256 checksums;
- registry-only consumer manifests, results, and ancestry evidence;
- tag and GitHub Release links;
- wheel workflow run IDs (TestPyPI and PyPI), aggregate manifest, and smoke
  result;
- the changelog and migration-guide diffs;
- the requirement-to-evidence matrix;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The code is already written and green; resist the temptation to improve it
here. Publication is manual and needs the maintainer's crates.io token and
`pypi` environment approval. Never claim "published" from a dry-run. If
the semver decision is contested, choose the higher (minor) version and
record the reasoning — over-declaring a behavior change is the recoverable
error.
