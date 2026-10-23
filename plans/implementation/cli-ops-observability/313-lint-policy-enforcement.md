# CLI, Ops, and Observability Milestone 313 — Lint-policy enforcement and Python-internal feature coverage

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md#milestone-313--lint-policy-enforcement`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (safe defaults; ownership boundaries)
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `docs/unsafe-code-policy.md` (the approved unsafe boundaries)
- `docs/dependency-policy.md`
- `architecture/eggserve-static.md`, `architecture/eggserve-server.md`
- `architecture/testing-and-conformance.md`
- `docs/public-api-boundary.md`

Primary class: invariant

## 1. Objective

Turn two policies the repository already states into mechanically enforced
rules, and bring one buildable code path under clippy.

1. **`missing_docs` is never enabled.** The workspace contains **zero**
   occurrences of `missing_docs` in any manifest or lint table. Every
   crate is a published API surface — `eggserve-core/src/lib.rs:43-46`
   promises "Before 1.0, every public type or function in this crate may
   change without a major version bump", and the crates are published on
   crates.io — yet nothing requires a public item to carry a doc comment.
2. **`undocumented_unsafe_blocks` is never enabled.** `docs/unsafe-code-policy.md`
   enumerates the *only* approved unsafe boundaries (the reviewed Windows
   FFI, the systemd descriptor-adoption path, and the test-fixture
   boundary), but nothing requires each `unsafe` block to carry a `SAFETY:`
   justification, so a new unreviewed `unsafe` block can be added without
   tripping any check.
3. **The `python-bindings-internal` feature is never linted.** It appears
   **zero** times in `.github/` and `scripts/`. The Python-only internal
   code path is buildable and entirely unchecked by clippy.

This milestone enables the two lints, remediates what they surface, and
adds a clippy lane for the Python-internal feature.

## 2. Why this milestone is ready

- The lints are standard `rustc`/`clippy` lints; no new tooling.
- The workspace is already `-D warnings` clean for clippy, so the
  remediation is bounded.
- The `python-bindings-internal` feature already exists and already
  compiles; it only needs a lane.
- No hard dependency on other open milestones.

## 3. Current implementation evidence

### 3.1 The lint surface today

`Cargo.toml` has a workspace `[workspace.lints]` table consumed by each
crate's `[lints] workspace = true`. It does **not** enable
`missing_docs` or `undocumented_unsafe_blocks`:

- `missing_docs`: 0 occurrences across `crates/` and the workspace
  manifest.
- `undocumented_unsafe_blocks`: 0 occurrences.

`cargo clippy --workspace --lib --bins --tests -- -D warnings` is clean
today, so enabling the lints surfaces only what those lints cover.

### 3.2 The published API surface

The crates published to crates.io are `eggserve-primitives`,
`eggserve-server`, `eggserve-static`, `eggserve-h3`, `eggserve-core`, and
`eggserve-bin`. `eggserve-primitives` is documented as the canonical
public contract (`architecture/primitives-api.md`), and
`docs/public-api-boundary.md` describes the boundary between the direct
crates and the compatibility umbrella. Doc comments are the only
machine-readable description of that surface that a consumer sees on
docs.rs.

### 3.3 The unsafe surface today

`crates/eggserve-static/src/fs/windows.rs` contains 42 `unsafe`
occurrences. The reviewed boundary is documented in
`docs/unsafe-code-policy.md` and in
`architecture/adr-002-windows-handle-relative-filesystem.md`. Other
approved boundaries exist for the systemd descriptor-adoption path and the
test-fixture boundary.

The `undocumented_unsafe_blocks` lint requires each `unsafe` block to
carry a `SAFETY:` comment. Whether that many blocks already do is not
known at the baseline and is the first thing the implementing agent must
measure.

### 3.4 The Python-internal feature

`crates/eggserve-static/Cargo.toml` declares:

```toml
[features]
python-bindings-internal = []
```

and `crates/eggserve-core/Cargo.toml` forwards it:

```toml
python-bindings-internal = ["eggserve-static/python-bindings-internal"]
```

with the comment that it exists so the Python bindings can keep a
crate-private path into the static authority without making it public.

Neither `.github/workflows/ci.yml` nor `scripts/verify.sh` mentions the
feature, so the code it enables is never clippy-linted and never
compiled by a CI lane.

### 3.5 The lint policy the project already states

`AGENTS.md` and `docs/unsafe-code-policy.md` state the rules; this
milestone makes them executable:

- "Unsafe Rust denied by default. Only the reviewed Windows FFI,
  systemd descriptor-adoption, and test-fixture boundaries in
  `docs/unsafe-code-policy.md` are allowed."
- `docs/dependency-policy.md`: "Every dependency needs an explicit
  purpose."

## 4. Invariants that must not regress

- **No public API change.** Enabling `missing_docs` must result in *added*
  documentation, never in a removed or renamed public item. If an item
  should not be public, that is a separate API decision requiring its own
  plan and a semver classification — this milestone must not make that
  call silently.
- No `unsafe` block is added, removed, or restructured by this milestone.
  The only change is the addition of `SAFETY:` comments where they are
  missing, and the comments must state the real invariant, not a
  placeholder.
- The approved unsafe boundary set in `docs/unsafe-code-policy.md` is
  unchanged.
- The `python-bindings-internal` feature keeps its existing semantics: it
  must not widen the static crate's public API, and the crate-private
  path it enables must remain crate-private.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings` stays
  green.
- The `cargo fmt --all -- --check` gate stays green.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.
- No published artifact changes without a separate publication milestone.

## 5. Scope

### In scope

- Enable `missing_docs` for the workspace, in the same lint table the
  crates already consume.
- Enable `undocumented_unsafe_blocks` for the workspace.
- Remediate: add the required doc comments and `SAFETY:` comments.
- Add a clippy lane for `--features python-bindings-internal` (and the
  equivalent for the excluded Python crate if it can carry the feature)
  in `.github/workflows/ci.yml` and in `scripts/verify.sh`.
- Document the lint policy in `architecture/testing-and-conformance.md` and
  in the contributor-facing guidance so that adding a public item or an
  `unsafe` block without the required comment is an expected CI failure.
- Record any item that cannot reasonably be documented or justified, as a
  named finding rather than a blanket `#[allow]`.

### Explicitly out of scope

- **No public API reduction.** Items that "should not be public" are
  recorded as findings and planned separately; they are not made private
  here.
- No change to any `unsafe` block's logic.
- No change to the approved unsafe boundary set.
- No change to the `python-bindings-internal` feature's meaning or its
  effect on the static crate's public surface.
- No dependency changes (the dependency-specification consolidation is
  Milestone 315).
- The cross-platform compile milestone (312) and the fuzz-harness repair
  (Milestone 310).
- Publication.

## 6. Required production changes

### Crates and ownership

- Workspace `Cargo.toml`: add `missing_docs` and
  `undocumented_unsafe_blocks` to the `[workspace.lints]` table, at the
  same level as the lints already there (the crates consume
  `[lints] workspace = true`).
- `crates/*/src/**`: added doc comments and `SAFETY:` comments only.
- `.github/workflows/ci.yml`, `scripts/verify.sh`: the
  `python-bindings-internal` clippy lane.
- `architecture/testing-and-conformance.md`, `AGENTS.md` (or the skill
  file, as the agent-facing surface): the lint policy statement.

No new crate. No dependency added or removed.

### Config and policy

- The lints must be enabled workspace-wide, not per crate, so that a new
  crate inherits them automatically.
- The lint level must match the workspace's existing `-D warnings` policy:
  a warning that CI treats as an error should be configured as a warning
  in the lint table, not as `deny`, so that the existing clippy invocation
  remains the single enforcement point.
- `python-bindings-internal` must be linted with the same strictness as
  the default feature set.

- **Recorded material deviation (resolved).** This plan named the second lint
  `undocumented_unsafe_blocks` and stated that it "is never enabled". There is
  no lint by that name: neither `rustc` nor `rustdoc` knows it, and on the
  current toolchain (1.99.1) both reject it as an unknown lint.

  ```
  $ rustc --crate-type lib -W undocumented_unsafe_blocks test.rs
  warning[E0602]: unknown lint: `undocumented_unsafe_blocks`
  $ rustdoc --crate-type lib -W rustdoc::undocumented_unsafe_blocks test.rs
  warning[E0602]: unknown lint: `rustdoc::undocumented_unsafe_blocks`
  ```

  The lint that actually enforces the policy is the **clippy** lint
  `clippy::undocumented_unsafe_blocks`, a restriction lint that requires a
  safety comment on the line preceding each `unsafe` block:

  ```
  warning: unsafe block missing a safety comment
   --> src/lib.rs:1:41
    |
  1 | pub unsafe fn f(p: *const i32) -> i32 { unsafe { *p } }
    |                                         ^^^^^^^^^^^^^
    = help: consider adding a safety comment on the preceding line
  ```

  It therefore belongs in `[workspace.lints.clippy]`, not
  `[workspace.lints.rust]`, and it is enforced by the clippy invocation
  rather than by `rustdoc`. The requirement is unchanged; only the lint's
  identity and its enforcing tool differ from what this plan assumed.

  Two consequences follow, and both change the shape of the work:

  1. The lint only fires on code the compilation unit actually compiles. On
     Linux the only compiled `unsafe` blocks are the two systemd
     descriptor-adoption blocks in
     `crates/eggserve-core/src/server/listener.rs` (both already carry a
     `SAFETY:` comment). The **35** Windows FFI blocks in
     `crates/eggserve-static/src/fs/windows.rs` are behind `#[cfg(windows)]`
     and never compile on Linux, so on Linux the lint would cover 2 of the
     repository's 37 unsafe blocks. Enforcing the policy therefore requires
     a clippy lane that compiles the Windows confinement code — the
     `platform-compile` job's Windows leg from Plan 312 is the natural home.
  2. `clippy::missing_safety_doc` (warn by default) additionally requires a
     `# Safety` section on every **public** `unsafe` function. That lint
     already applies today, so the remediation must not regress it.

  The clippy lint name is `clippy::undocumented_unsafe_blocks`; the bare name
  in this plan resolves only inside clippy, and clippy itself warns that it
  "is deprecated and may not have an effect in the future".


### Protocol and compatibility

None. No wire or API behavior change.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and confinement

- `undocumented_unsafe_blocks` is the mechanical form of
  `docs/unsafe-code-policy.md`. Every `SAFETY:` comment must name the
  invariant that makes the block safe, so that a later edit which
  invalidates the invariant is visible in review.
- A `SAFETY:` comment must not be added to a block outside the approved
  boundaries. If the implementing agent finds an `unsafe` block outside
  the policy, that is a **finding** to report — not a block to bless with
  a comment.

### Documentation and static guards

- `architecture/testing-and-conformance.md` — the enforced lint policy.
- `docs/unsafe-code-policy.md` — a pointer to the mechanical enforcement
  (the policy text itself is unchanged).
- `.github/workflows/ci.yml`, `scripts/verify.sh` — the new lane.
- `AGENTS.md` / `.opencode/skills/eggserve-dev/SKILL.md` — the
  agent-facing statement that these lints are enforced.

## 7. Ordered work packages

### Work package A — Measure the remediation surface

Before enabling anything, run the lints in report-only mode and count:

- the number of undocumented public items per crate;
- the number of `unsafe` blocks without a `SAFETY:` comment, per file;
- the number of `unsafe` blocks outside the approved boundaries in
  `docs/unsafe-code-policy.md`.

Acceptance evidence: a measured table recorded in the closure record.

### Work package B — Remediate the unsafe blocks

Add a `SAFETY:` comment to each `unsafe` block that lacks one. Each
comment must state the invariant that justifies the block (for example:
which handle stays valid for the duration of the call, which pointer
remains owned, which value was validated before the call).

If a block's justification cannot be written accurately, that is a finding
about the block, not a license to write a vague comment.

Acceptance evidence: every `unsafe` block has an accurate `SAFETY:`
comment; the count of blocks outside the approved boundaries is reported
separately.

### Work package C — Remediate the undocumented public items

Add doc comments to the undocumented public items. Each comment must be
useful, not merely present — the lint accepts any doc comment, so the
implementing agent must review the added comments rather than
auto-generating them.

Where an item's correct documentation depends on a semantic decision
outside this milestone's scope, record it as a finding and add a minimal
but accurate comment.

Acceptance evidence: `missing_docs` is clean with `-D warnings`, and a
reviewer confirms the added comments are meaningful.

### Work package D — Enable the lints

Add both lints to the workspace `[workspace.lints]` table and verify the
existing clippy invocation turns them into errors.

Confirm the new crates inherit the table automatically.

Acceptance evidence: `cargo clippy --workspace --lib --bins --tests -- -D
warnings` is clean, and a deliberate undocumented public item fails the
build.

### Work package E — Add the Python-internal clippy lane

Add a clippy (and, where cheap, a compile) lane for
`--features python-bindings-internal` in `.github/workflows/ci.yml` and
`scripts/verify.sh`.

The lane must cover the excluded Python crate too, because the feature
exists for the Python bindings.

Fix whatever the lane surfaces, without changing the feature's meaning.

Acceptance evidence: the lane runs in CI; a deliberate warning in the
feature-gated code fails it.

### Work package F — Documentation

Update `architecture/testing-and-conformance.md`, `docs/unsafe-code-policy.md`
(cross-reference only), and the agent-facing guidance.

Acceptance evidence: the documentation states that the lints are enforced
by CI and what a contributor must do when a lint fires.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable; no runtime change.

The lint lane must fail the job on a warning; no `continue-on-error`, and
no blanket `#[allow(missing_docs)]` on a module to silence a count.

## 9. Compatibility and migration

- No public API change and no semver event.
- Doc comments are additive and visible on docs.rs; a consumer reading the
  published docs sees more, not less.
- `SAFETY:` comments are internal to the source and are not published
  behavior.
- The `python-bindings-internal` feature's meaning is unchanged.

## 10. Required tests

### Focused unit tests

None required; the change is comments plus lint configuration.

### Integration tests

None required.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- Every `unsafe` block carries an accurate `SAFETY:` comment (verified by
  the lint plus a spot review).
- Any `unsafe` block found outside the approved boundaries is reported as
  a finding with its severity, and is not silently blessed.

### Migration and compatibility tests

- `cargo clippy --workspace --lib --bins --tests -- -D warnings` is clean.
- The `python-bindings-internal` lane is clean.
- `cargo fmt --all -- --check` is clean.
- The topology gate and its `--self-test` pass.

## 11. Required verification commands

```bash
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-static --features python-bindings-internal --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features python-bindings-internal --lib --tests -- -D warnings
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo fmt --all -- --check
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
./scripts/verify.sh fast
```

## 12. Documentation updates

- `architecture/testing-and-conformance.md` — the enforced lint policy.
- `docs/unsafe-code-policy.md` — a cross-reference to the mechanical
  enforcement; the policy text is unchanged.
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` — the
  agent-facing statement that these lints are enforced.
- `plans/subsystems/cli-ops-observability-roadmap.md` — Milestone 313
  status.
- `plans/registry.md` — the milestone row.
- `plans/closure/cli-ops-observability/313-lint-policy-enforcement.md`
  (new).

## 13. Acceptance criteria

- `missing_docs` and `undocumented_unsafe_blocks` are enabled in the
  workspace lint table and enforced by the existing clippy invocation.
- The workspace is clean under both lints with `-D warnings`.
- Every `unsafe` block carries an accurate `SAFETY:` comment, and any
  block outside the approved boundaries is reported as a finding.
- The `python-bindings-internal` feature is clippy-linted in CI and in
  `scripts/verify.sh`.
- No public item was removed, renamed, or made private.
- No `unsafe` block's logic changed.
- The documented lint policy matches the enforced policy.

## 14. Stop conditions

Stop and report rather than improvise if:

- enabling `missing_docs` surfaces a public item whose correct
  documentation requires an API decision (a rename, a removal, a
  visibility change) — record it as a finding and plan it separately;
  do not make the API decision here;
- an `unsafe` block outside the approved boundaries of
  `docs/unsafe-code-policy.md` is found — report it with severity and
  evidence; do not bless it with a `SAFETY:` comment;
- a `SAFETY:` comment cannot be written accurately for a block, which
  means the block's justification is unknown — report it;
- the `python-bindings-internal` lane surfaces findings that require
  changing the feature's meaning or widening the static crate's public
  API — report it;
- a blanket `#[allow]` would be needed to make the lints pass — that is a
  stop condition, not a solution.

## 15. Closure evidence required

Create `plans/closure/cli-ops-observability/313-lint-policy-enforcement.md`
containing:

- the measurement table from Work package A;
- the lint configuration diff;
- the `unsafe`-block remediation summary and the separate list of blocks
  outside the approved boundaries, with severities;
- the documentation-remediation summary, with a spot-review note on the
  quality of the added comments;
- the `python-bindings-internal` CI and `verify.sh` diffs plus the
  lane's output;
- the demonstration that a deliberate violation fails;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The lint counts are the honest first deliverable — measure before
remediating, because the size of the job determines whether this milestone
needs splitting. The two failure modes to avoid are blanket `#[allow]`
attributes (which would make the lints decorative) and `SAFETY:` comments
that restate the code rather than stating the invariant. If the remediation
surface turns out to be very large, split the milestone by crate and
record the split rather than shipping a blanket allow.
