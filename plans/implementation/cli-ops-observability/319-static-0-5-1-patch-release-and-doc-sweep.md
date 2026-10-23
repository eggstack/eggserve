# Plan 319 — `eggserve-static` 0.5.1 patch release and current-line documentation sweep

## 1. Status

- **State**: active — implementation in progress
- **Plan number**: 319
- **Subsystem**: CLI, ops, and observability (`plans/subsystems/cli-ops-observability-roadmap.md`)
- **Source roadmap**: `plans/subsystems/cli-ops-observability-roadmap.md`
- **Primary class**: release-operations + documentation-accuracy

## 2. Repository baseline

At the start of this plan the repository carries the Plan 317 and Plan 318
changes, with their closure records written.

Two things are outstanding:

1. **`eggserve-static` 0.5.1 is owed.** Plan 316 and Plan 317 both changed
   `eggserve-static` after the `v0.5.0` tag was published on 2026-10-22. The
   published `eggserve-static` 0.5.0 still carries the NT directory-buffer
   defect batch: the hand-rolled FFI struct with wrong offsets on every
   field, `get_file_id` reading the low half of `CreationTime` instead of
   the stable `FileId`, an undersized record buffer, and a zero entry budget
   that returned one entry instead of none.

   `eggserve-static` is independent of the Plan 318 floor raise: it does not
   constrain `rustls` (neither does `eggserve-primitives`), so its patch does
   not wait on the republication the floor raise creates.

2. **The current line is misdescribed in the normative documentation.**
   `docs/release-contract.md` — the normative reference for what EggServe
   ships — still states the per-crate versions of the superseded `0.4.0`
   line and says the current tree is on the `0.4.x` release line. Several
   `architecture/` deep dives do the same (`eggserve-h3.md`,
   `eggserve-primitives.md`, `eggserve-server.md`, `eggserve-bin.md`,
   `overview.md`). Plan 314 swept the *pending-publication* statements but
   not the *current-version* strings, so these surfaces were missed.

## 3. Problem statement

A release contract that names the wrong versions is worse than no contract:
it is normative, it is read as current, and it tells a consumer which crate
versions exist. The `0.5.0` line has been published since 2026-10-22 and the
contract still describes the line it superseded.

Separately, the `eggserve-static` patch is owed: a published crate still
carries a defect batch that the repository fixed, and the fix is not
reachable by a consumer until a new version is published.

## 4. Goal

Make the normative documentation describe the published line, and publish the
`eggserve-static` patch that carries the NT directory-buffer fixes.

## 5. Non-goals

- Not a republication of the crates whose manifests the Plan 318 floor raise
  changed. That republication is a separate release line with its own plan.
- Not a wheel release. The wheel is unchanged by this plan.
- Not a change to any confinement decision, path policy, or public API.
- Not a documentation rewrite. This plan corrects version statements that are
  wrong; it does not restyle normative prose.

## 6. Scope

### Work package A — Correct the current-line version statements

Update the version strings and the "current line" statements in:

- `docs/release-contract.md` — the per-crate versions and the release-line
  sentence.
- `architecture/eggserve-h3.md`, `architecture/eggserve-primitives.md`,
  `architecture/eggserve-server.md`, `architecture/eggserve-bin.md`,
  `architecture/overview.md`, `architecture/eggserve-static.md`,
  `architecture/eggserve-core.md`, `architecture/eggnet-tls.md` — where a
  *current* claim names a superseded version.

Historical statements ("Plan 300 moved the line to … 0.4.0") are correct as
history and are left alone.

### Work package B — Bump `eggserve-static` to 0.5.1

- `crates/eggserve-static/Cargo.toml` → `0.5.1`.
- The dependent crates' caret requirements (`eggserve-core`,
  `eggserve-bin` → `eggserve-static = "0.5.0"`) already accept `0.5.1`;
  they are left alone, because a caret requirement is not a pin and bumping
  it would narrow the accepted range for no reason.
- The lockfiles are regenerated, which is the one legitimate lockfile change
  in this line of work.

### Work package C — Prepare and publish

- The changelog entry for the patch.
- `bash scripts/verify-cargo-packages.sh --mode all` — the layered package
  dry-run that proves the crate packages and installs cleanly.
- `cargo publish -p eggserve-static --locked --dry-run`, then the publish.
- Post-publication verification: the published version resolves from the
  registry and the registry-only consumer proof installs against it.

## 7. Invariants that must not regress

1. **No public API change.** `eggserve-static` 0.5.1 is a patch release:
   source compatibility is preserved. `parse_directory_buffer` was already
   public in 0.5.0 and its documented contract already stated that a zero
   budget yields an empty listing, so the fix makes the code match its
   published documentation rather than changing the contract.
2. **The dependency graph does not move.** No crate gains, loses, or upgrades
   a dependency; only `eggserve-static`'s own version moves.
3. **The normative documentation names the published line.**
4. **Publication is the last step, not the first.** Nothing is published
   before the package dry-run and the workspace suite are green.

## 8. Recorded material deviation

None yet.

## 9. Verification strategy

- `cargo fmt --all -- --check`,
  `cargo clippy --workspace --lib --bins --tests -- -D warnings`,
  `cargo test --workspace`.
- `bash scripts/verify-cargo-packages.sh --mode all`.
- `cargo publish -p eggserve-static --locked --dry-run`.
- `python3 scripts/check-crate-topology.py` and `--self-test`.
- Post-publication: the crates.io API shows `eggserve-static 0.5.1`, and a
  registry-only consumer resolves it.

## 10. Stop conditions

Stop and escalate rather than continue if any of the following holds.

- The package dry-run fails, or the publish is rejected by crates.io.
- The workspace suite is not green.
- Bumping `eggserve-static` forces a change in any dependent crate's
  requirement, which would mean the requirements are pins rather than carets.

## 11. Risks and decision points

- **Risk**: publishing `eggserve-static` 0.5.1 while the floor raise is
  unpublished leaves the published line split across two floors for a short
  window. That is acceptable: the floor raise is a *widening* of what the
  crates accept, so the published crates remain resolvable and secure at
  their own floors, and the static patch does not depend on it.
- **Decision point**: whether to publish the static patch before or after
  the floor-raise republication. Publishing the static patch first is
  chosen because the two releases are independent and the static patch is
  already owed; batching them would delay the fix without reducing risk.

## 12. Dependencies

- Plan 316 and Plan 317 produced the `eggserve-static` changes this release
  carries.
- Plan 318 raised the floor in the manifests of the crates that constrain
  `rustls`; `eggserve-static` is not one of them, so this release is
  independent of it.

## 13. Closure requirements

A closure record under `plans/closure/cli-ops-observability/` recording: the
version statements corrected, the version bump, the dry-run evidence, the
publication evidence (crates.io version, timestamp, checksum), and the
registry-only consumer proof.

## 14. Implementation notes

Recorded as implementation proceeds.

## 15. Verification evidence

Recorded as verification proceeds.

## 16. Rollback

The documentation corrections and the version bump are independent of the
publication. A published crates.io version cannot be rolled back — only
superseded by a new version — so the bump and the publish are treated as one
irreversible step, taken only after the dry-run is green.
