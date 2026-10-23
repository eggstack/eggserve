# CLI, Ops, and Observability Milestone 315 — Dependency specification consolidation

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md#milestone-315--dependency-specification-consolidation`

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `docs/dependency-policy.md` ("No broad dependencies. Every dependency needs an explicit purpose.")
- `docs/unsafe-code-policy.md`
- `deny.toml`, `audit.toml`
- `docs/release-process.md`
- `AGENTS.md` ("Direct `rustls` constraints carry a `0.23.45` caret floor
  (RUSTSEC-2026-0285) in every constraining manifest including the excluded
  Python crate — never roll it back.")
- `architecture/crate-topology.md` (direct-graph purity rules)

Primary class: polish

## 1. Objective

Declare shared dependency versions once so that advisory floors and shared
versions cannot drift between manifests.

At the baseline there is **no `[workspace.dependencies]` table**. Every
crate declares its own versions, and the advisory-sensitive `rustls`
specification is restated verbatim in **six** manifests:

| Manifest | Line | Declaration |
|---|---|---|
| `crates/eggnet-tls/Cargo.toml` | 25 | `rustls = { version = "0.23.45", default-features = false, features = ["std", "ring"] }` |
| `crates/eggserve-server/Cargo.toml` | 49 | same |
| `crates/eggserve-static` | — | (no rustls) |
| `crates/eggserve-h3/Cargo.toml` | 37 | same |
| `crates/eggserve-core/Cargo.toml` | 64 | same, plus `optional = true` |
| `crates/eggserve-bin/Cargo.toml` | 52 | same |
| `crates/eggserve-python/Cargo.toml` | 47 | same (excluded crate) |

The repository's own agent guidance warns about exactly this risk:
"Direct `rustls` constraints carry a `0.23.45` caret floor
(RUSTSEC-2026-0285) in every constraining manifest including the excluded
Python crate — never roll it back." Today, satisfying that instruction
means editing six places, and the excluded Python crate cannot use a
workspace table at all.

This milestone introduces a `[workspace.dependencies]` table, points the
workspace members at it, and keeps the excluded Python crate's explicit
specifications — **without changing any resolved version**.

## 2. Why this milestone is ready

- The change is manifest metadata only; no source code changes.
- The hard acceptance criterion — both lockfiles byte-identical before and
  after — makes it verifiable and non-regressive.
- No new dependency is added and none is removed.
- No hard dependency on other open milestones.

## 3. Current implementation evidence

### 3.1 The workspace manifest

`Cargo.toml` has `[workspace.package]` (with `version = "0.2.4"`,
`edition = "2021"`, `license`, `repository`, `homepage`, `readme`,
`description`, `keywords`, `categories`, `rust-version`) and `[workspace]`
with `members` (7 crates), `exclude` (`crates/eggserve-python`), and
`resolver = "2"`.

There is **no `[workspace.dependencies]` table**, and no crate uses
`workspace = true` for a dependency.

### 3.2 The excluded crate constraint

`crates/eggserve-python` is listed in `exclude` and has its own
`Cargo.lock`. A workspace member can use `workspace = true` for a
dependency only if that dependency is declared in the workspace table;
an excluded crate **cannot** use the workspace's table at all.

Therefore the `rustls` `0.23.45` floor must remain stated literally in
`crates/eggserve-python/Cargo.toml`, and `AGENTS.md`'s instruction that
the floor be present "in every constraining manifest including the
excluded Python crate" remains satisfiable — but only by keeping that one
manifest explicit.

### 3.3 The advisory floor

`crates/eggserve-core/Cargo.toml:61-65` carries the rationale comment:

```
# TLS dependencies (feature-gated)
# Security floor (Plan 218, RUSTSEC-2026-0285): caret floor keeps Cargo from
# resolving below the patched 0.23.45 line.
rustls = { version = "0.23.45", optional = true, default-features = false, features = ["std", "ring"] }
```

The same floor appears without that comment in the other five manifests.
`deny.toml` bans wildcards and `native-tls`/`openssl-sys`; `aws-lc-rs` is
intentionally allowed (dev-only `rcgen` unification), and the production
tree is ring-only.

### 3.4 The observed duplication

Beyond `rustls`, the following shared specifications are repeated across
manifests with divergent feature sets:

| Dependency | Declarations | Note |
|---|---|---|
| `rustls` | 6 | identical version and features (plus `optional` in core) |
| `tokio-rustls` | 5 | feature sets differ (`["ring"]` vs none) |
| `tokio` | 6+ | feature sets differ per crate |
| `hyper` | 3+ | feature sets differ per crate |
| `httpdate` | 2 | identical |
| `rustls-pki-types` | 3 | identical |
| `phf` | 1 | static only |
| `rustix` | 2 | static (`fs`,`net`) vs core (`net`) |

Cargo's `workspace = true` **inherits the version** but a crate may add
its own features on top (`{ workspace = true, features = [...] }`).
Feature sets that genuinely differ per crate therefore cannot be fully
centralized, and this milestone must not try.

## 4. Invariants that must not regress

- **No resolved dependency version changes.** Both `Cargo.lock` files must
  be byte-identical before and after. Any lockfile delta is a stop
  condition.
- The `rustls` `0.23.45` caret floor (RUSTSEC-2026-0285) remains present
  and effective in every constraining manifest, **including the excluded
  Python crate**, whose explicit declaration must not be removed.
- `deny.toml` stays green for both lockfiles: no wildcards, no
  `native-tls`/`openssl-sys`, `aws-lc-rs` dev-only, production tree
  ring-only.
- No dependency is added, removed, or upgraded. This milestone is a
  specification-layout change only.
- The direct dependency graph stays free of `eggserve-core`,
  `eggserve-static`, and PHF (`scripts/check-crate-topology.py:171-185`),
  and the Plan 276/291 registry-only consumer ancestry proof remains
  valid.
- The excluded Python crate keeps its own `Cargo.lock` and its exclusion
  from the workspace.
- `cargo publish --locked --dry-run` remains clean for every published
  crate: the published manifests must still resolve independently, and
  `workspace = true` dependencies must be materialized by `cargo publish`.
- The `scripts/check-supply-chain.sh` audit of **both** lockfiles stays
  green.
- No source file changes.

## 5. Scope

### In scope

- Add a `[workspace.dependencies]` table to the workspace `Cargo.toml`
  covering the specifications that are genuinely shared and identical
  across crates.
- Point the workspace members at it with `workspace = true`, adding
  crate-specific features on top where the feature sets differ.
- Keep `crates/eggserve-python` (excluded) fully explicit, including the
  `rustls` `0.23.45` floor and its rationale comment.
- Move the advisory rationale comment to the single place it now applies,
  and make sure every remaining explicit declaration (notably the
  excluded crate's) still carries or references the floor rationale.
- Verify both lockfiles are byte-identical before and after.
- Re-run the supply-chain audits for both lockfiles and the per-crate
  publish dry-runs.
- Add a small static guard (script or gate rule) that fails if a
  constraining manifest drops the `rustls` floor below `0.23.45`.
- Document the layout in `docs/dependency-policy.md`.

### Explicitly out of scope

- **No version change of any kind.** If centralizing a dependency would
  resolve differently, that dependency stays explicit.
- No dependency added, removed, upgraded, or downgraded.
- No feature unification. A crate that needs `tokio` features another
  crate does not keeps its own feature list.
- No attempt to make the excluded Python crate use the workspace table.
- No change to `deny.toml` or `audit.toml`.
- The lint-policy milestone (313) and the cross-platform compile
  milestone (312).
- The fuzz-harness repair (Milestone 310).
- Publication.

## 6. Required production changes

### Crates and ownership

- `Cargo.toml`: gains a `[workspace.dependencies]` table.
- `crates/{eggnet-tls,eggserve-primitives,eggserve-server,eggserve-static,eggserve-h3,eggserve-core,eggserve-bin}/Cargo.toml`:
  shared specifications become `{ workspace = true }` (plus crate-specific
  features where needed).
- `crates/eggserve-python/Cargo.toml`: unchanged in substance; it keeps
  explicit declarations and its own lockfile.
- `scripts/check-supply-chain.sh` or `scripts/check-crate-topology.py`: the
  floor guard.
- `docs/dependency-policy.md`: the layout and the guard.

No new crate. No source file changes.

### Config and policy

- The workspace table declares versions only. Feature sets stay in the
  crates that need them, because Cargo's inheritance model adds features
  rather than replacing them, and per-crate feature sets here are
  intentionally different (for example `tokio-rustls` with `["ring"]`
  where a crate needs it and without where it does not).
- `default-features = false` must be preserved wherever it is set today.
  This is the single most likely way to introduce a silent resolution
  change, and the lockfile comparison is the guard.

- **Recorded material deviation (resolved).** This plan originally stated that
  "Cargo's workspace inheritance does **not** carry `default-features`; a crate
  that needs `default-features = false` must state it explicitly." That is
  backwards, and following it literally would have changed the resolved
  feature set of every member.

  Cargo **does** inherit `default-features` from the workspace table. The
  failure mode is the opposite one: when a table entry leaves
  `default-features` unspecified (that is, defaults **on**) and a member asks
  for `default-features = false`, the member-level request is **ignored** and
  Cargo emits

  ```
  warning: `default-features` is ignored for <dep>, since `default-features`
  was not specified for `workspace.dependencies.<dep>`; overriding workspace
  `default-features` to false requires Rust 1.99+ and the 2024 edition
  ```

  Overriding to `false` from the table requires Rust 1.99 and edition 2024;
  this repository is edition 2021 with MSRV 1.89, so that override would not
  have been honored on the MSRV lane at all.

  The resolution: the table states `default-features` explicitly wherever the
  members do not agree on the default set — `rustls` and `tokio-rustls` both
  carry `default-features = false` in the table — and the members that need
  the default feature set restate `default-features = true`, which Cargo
  honors. A member restating `default-features = false` against a table entry
  that already says `false` is then a consistency statement rather than an
  override, and is honored on every supported toolchain.

  The plan's §3.4 inventory also needed correcting: `rustls` is constrained by
  **five** workspace members (`eggnet-tls`, `eggserve-h3`, `eggserve-core`, and
  the `eggserve-server` and `eggserve-bin` **dev**-dependencies) plus the
  excluded crate; the plan's table described `eggserve-server`'s declaration
  as a normal dependency and omitted `eggserve-h3` from
  `docs/dependency-policy.md`'s own floor list.

  Because the lockfile records versions and not features, the lockfile
  comparison alone does **not** prove this milestone. The real guard is the
  resolved feature tree, which was diffed before and after and is empty.


### Protocol and compatibility

None. No runtime behavior change.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

- The `rustls` floor is the supply-chain security floor for RUSTSEC-2026-0285.
  It must remain effective in every constraining manifest.
- A guard must fail if any constraining manifest drops the floor. The
  guard must also cover the excluded Python crate, which is the one place
  the workspace table cannot help.

### Documentation and static guards

- `docs/dependency-policy.md` — the workspace-table layout, the excluded
  crate's explicit exception, and the floor guard.
- `scripts/` — the floor guard.

## 7. Ordered work packages

### Work package A — Inventory the duplicated specifications

Produce the full inventory of dependency specifications across the eight
manifests: name, version, feature set, and the set of manifests that
declare it. Classify each as:

- **identical** — same version and same feature set everywhere
  (candidates for the workspace table);
- **version-shared, feature-divergent** — same version, different features
  (version goes in the table, features stay in the crate);
- **genuinely distinct** — different versions (stay explicit).

Acceptance evidence: the inventory table is recorded in the closure record.

### Work package B — Add the workspace table

Add `[workspace.dependencies]` with the version entries from the
"identical" and "version-shared" classes. Do not include `default-features`
in the table unless every consumer wants it; Cargo's inheritance model
makes `default-features` a per-crate decision.

Acceptance evidence: the table exists and every entry corresponds to an
inventory row.

### Work package C — Convert the workspace members

Replace the duplicated declarations with `{ workspace = true }` (plus
crate-specific features where the inventory says they differ), preserving
`optional = true`, `default-features = false`, and target-specific
placement exactly as they are today.

Do not convert a declaration whose `default-features` behavior cannot be
preserved by the inheritance model.

Acceptance evidence: `git diff` of the manifests shows only the intended
substitutions, with no removed feature.

### Work package D — Verify the lockfiles are unchanged

Capture both lockfiles' hashes before the change, apply the change, and
compare after.

Both `Cargo.lock` (workspace) and `crates/eggserve-python/Cargo.lock`
(excluded crate) must be byte-identical.

If either differs, the change altered resolution: revert the offending
entry and record why.

Acceptance evidence: the before/after hashes, and a `git diff` over the
lockfiles showing no change.

### Work package E — Floor guard

Add a static check that fails if any constraining manifest — including
`crates/eggserve-python/Cargo.toml` — declares `rustls` with a version
requirement below `0.23.45`, or omits it while declaring the `tls`/`http3`
features.

Place it where the other supply-chain gates live and wire it into
`scripts/check-supply-chain.sh` or the topology gate's rule set, so it
runs in routine CI.

Acceptance evidence: the guard exists, runs in CI, and demonstrably fails
when the floor is lowered in a scratch copy.

### Work package F — Re-run the supply-chain and packaging gates

- `bash scripts/install-cargo-tools.sh`
- `bash scripts/check-supply-chain.sh` (both lockfiles)
- `bash scripts/verify-cargo-packages.sh --mode all`
- `cargo publish -p <each published crate> --locked --dry-run`

`cargo publish --dry-run` is the critical one: it proves the published
manifest materializes the inherited versions correctly.

Acceptance evidence: every gate green.

### Work package G — Documentation

Update `docs/dependency-policy.md` with the layout, the rule for when a
version belongs in the table, the excluded crate's exception, and the
floor guard.

Acceptance evidence: the policy states the layout and the guard.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable; no runtime change.

The lockfile comparison is the equivalent of a "no resolution change"
invariant: it must be treated as a hard gate, not a suggestion.

## 9. Compatibility and migration

- No consumer-visible change and no semver event: the published manifests
  resolve identically.
- The published crates' manifests change shape (inherited rather than
  literal), which is a metadata change with no resolved-version effect.
  `cargo publish --locked --dry-run` is the proof.
- `crates/eggserve-python` is unaffected.

## 10. Required tests

### Focused unit tests

None; manifest-only change.

### Integration tests

None; manifest-only change.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- The floor guard fails when the `rustls` floor is lowered in a scratch
  copy of a manifest.
- The supply-chain audit stays green for both lockfiles.
- `deny.toml` bans stay effective: no wildcard and no `native-tls`/
  `openssl-sys` appears in either lockfile.

### Migration and compatibility tests

- `cargo publish --dry-run --locked` succeeds for every published crate,
  proving the inherited versions materialize.
- Both lockfiles are byte-identical before and after.

## 11. Required verification commands

```bash
# Lockfile invariance
sha256sum Cargo.lock crates/eggserve-python/Cargo.lock   # before and after

# Gates
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
cargo publish -p eggserve-primitives --locked --dry-run
cargo publish -p eggserve-server --locked --dry-run
cargo publish -p eggserve-static --locked --dry-run
cargo publish -p eggserve-h3 --locked --dry-run
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-bin --locked --dry-run
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked

# Unaffected-workspace guards
cargo fmt --all -- --check
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo test --workspace
./scripts/verify.sh fast
```

## 12. Documentation updates

- `docs/dependency-policy.md` — the workspace-table layout, the excluded
  crate's exception, and the floor guard.
- `plans/subsystems/cli-ops-observability-roadmap.md` — Milestone 315
  status.
- `plans/registry.md` — the milestone row.
- `plans/closure/cli-ops-observability/315-dependency-specification-consolidation.md`
  (new).

## 13. Acceptance criteria

- A `[workspace.dependencies]` table exists and the workspace members use
  it for the genuinely shared specifications.
- `crates/eggserve-python/Cargo.toml` keeps explicit declarations,
  including the `rustls` `0.23.45` floor.
- Both lockfiles are byte-identical before and after.
- No dependency is added, removed, upgraded, or downgraded.
- The floor guard exists, runs in routine CI, and demonstrably fails when
  the floor is lowered.
- `check-supply-chain.sh` is green for both lockfiles, and every
  `cargo publish --locked --dry-run` is clean.
- `docs/dependency-policy.md` describes the layout and the guard.

## 14. Stop conditions

Stop and report rather than improvise if:

- either lockfile changes — a resolution change is out of scope and must
  be reverted, not absorbed;
- a dependency's `default-features` behavior cannot be preserved by
  Cargo's inheritance model — keep that declaration explicit and record
  why;
- the floor guard would have to be waived for any constraining manifest —
  report it;
- centralizing a version would make a published crate's manifest depend on
  a workspace table that `cargo publish` cannot materialize — keep it
  explicit and record it;
- any change to `deny.toml`, `audit.toml`, or the dependency set is
  tempting — that is a separate plan.

## 15. Closure evidence required

Create `plans/closure/cli-ops-observability/315-dependency-specification-consolidation.md`
containing:

- the dependency inventory table with its classification;
- the workspace-table diff;
- the per-manifest conversion diff;
- the before/after lockfile hashes for both lockfiles;
- the floor guard's source and its demonstration that it fails on a
  lowered floor;
- the supply-chain audit and publish dry-run results;
- the documentation diff;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The lockfile-invariance check is the whole safety story of this
milestone. If it changes at all, something about the inheritance model
resolved differently — most likely a `default-features` difference — and
the right move is to keep that dependency explicit rather than to accept
the new resolution.
