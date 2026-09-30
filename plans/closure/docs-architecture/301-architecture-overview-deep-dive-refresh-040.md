# Docs Architecture Milestone 301 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/docs-architecture/301-architecture-overview-deep-dive-refresh-040.md`

Source subsystem roadmap:

- `plans/subsystems/docs-architecture-roadmap.md#7`

Repository baseline reviewed: `main` post-Plan-299 (closed 294–299 code
state, Plan 300 active publication: `server`/`static`/`h3`/`core 0.4.0`,
`primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`).

Implementation commits or pull requests:

- (this commit) — docs-only refresh of `architecture/overview.md` + deep
  dives to the 0.4.0 baseline, plus Plan 301 registration.

## 1. Executive finding

The milestone's polish boundary is complete. `architecture/overview.md`
remains the bird's-eye index (2–4 sentence overviews per discrete
module/tool/capability with links to owning deep dives); all deep dives
were walked once via six parallel subagents against current code, and every
hard-stale 0.3.x claim found was corrected. No behavior, API, dependency,
or tier change. No open findings require a corrective code plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Overview crate sections verified to 0.4.0 | subagent groups 1–2 reports; `crates/*/Cargo.toml` versions checked | pass | Fixed `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`; added 294–297 + 299 F3 pointers |
| Overview tool map verified | subagent group 6 counts: 11 fuzz targets, 51/55(47)/17 corpora, 19 scripts, 26 arch files, 3 CI jobs | pass | Added 294–297 benchmark dirs + 4 server test names to Testing Strategy |
| Every deep dive walked once | 6 subagent reports covering all 23 deep dives + 2 ADRs | pass | Reports retained in session; findings below |
| Stale claims corrected | ~30 targeted doc edits across 15 files | pass | Listed in §3; line-number-only drift left as low findings |
| Links resolve | link-resolution check over `architecture/*.md` | pass | No new links added; all existing resolve |
| Invariants preserved | topology + conformance gates; docs-only diff | pass | No code touched |
| Tier labels consistent | H1 + primitives supported; remainder experimental kept | pass | http2 non-goals narrowed (tunnel via Extended CONNECT stays) |
| Registry + roadmap updated | `plans/registry.md`, `docs-architecture-roadmap.md` §12 | pass | 301 active → closed in this commit |

## 3. Production implementation evidence

Docs-only: no production-code changes. Doc corrections applied:

- `overview.md`: 0.4.0 version line + Plan 300 pointer, 293→301 plan context,
  server/core/bin/python crate rows, 294–297 benchmark index, Testing
  Strategy server-test names, 5-stage pipeline wording.
- `eggserve-server.md`: `0.4.0` version, optional `http`/`tower-service`/
  `tower-layer` deps + 296 WP-A edge.
- `eggserve-core.md`: `0.4.0` line + 296 WP-A note.
- `eggserve-bin.md`: `0.2.2` + Plan 300 publication note.
- `eggserve-python.md`: wheel `0.2.4` + `0.4.0` leaves.
- `eggserve-primitives.md`: `0.2.2` version note.
- `eggserve-h3.md`: `0.4.0` version note.
- `eggserve-static.md`: `mime.rs` private note, `extra_response_headers`
  every-response correction.
- `primitives-api.md`: `Vec<ListingEntry>`, `metadata()`/`is_empty()`.
- `path-confinement.md`: `TooLong` static-400 vs runtime-414 split.
- `error-taxonomy.md`: five-layers-plus-hook header, 5-stage wording,
  `AbsolutePath` reserved.
- `runtime.md`: core-owned selection prefix, H1-only `driver.rs` row,
  upgrades-enabled correction.
- `http2.md`: non-goals narrowed (no `Upgrade: h2c`; tunnel via Extended CONNECT).
- `configuration.md`: listing-bytes upper bound, `StaticPolicy.symlinks`
  naming, compatibility-only TLS scoping, submodule visibility.
- `testing-and-conformance.md`: 294–297 benchmarks + 3 server suites + F3.
- `structured-logging.md`: selected-set qualifiers, direct-vs-compat
  `StaticService` fix.
- `filesystem-confinement.md`: `reject_backslash` field.
- `crate-topology.md`: current `0.4.0` line (288–291 history kept).

## 4. Verification executed

### Commands run

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
```

### Results

- Conformance matrix: pass (51 entries, 55 scenarios/47 routine, 17 H3).
- Topology gate: pass.
- `cargo fmt --check`: pass.
- `verify.sh fast`/`full` not run: docs-only change with no code impact.

## 5. Invariant review

- Safe defaults / no-serving-outside-root / static authority / H1 authority /
  QUIC-behind-`http3`: untouched (docs-only).
- `OpsContext` logging, one-shot bodies, framing ownership: claims
  re-verified, unchanged.
- Overview remains the index; every link resolves.

## 6. Failure and recovery review

Docs-only: no runtime failure modes. All deep dives walked. Code/doc
contradictions found were all doc-stale (code wins); none required stopping.

## 7. Migration and compatibility review

No compatibility effect. Version strings at manifests-proven values
(`server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`,
wheel `0.2.4`, `eggnet-tls` workspace `0.2.4`, PyO3 `0.29.2`).

## 8. Security review

No confinement/policy change. `security-model.md` untouched; path-stage
wording aligned to the 5-stage + construction form.

## 9. Documentation and operations

Updated: `architecture/overview.md` + 14 deep dives (list in §3),
`plans/subsystems/docs-architecture-roadmap.md` (§12),
`plans/registry.md` (301 active), this closure record. No `docs/`
normative edits. No guard-script changes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `filesystem-confinement.md` Unix/Windows line numbers drifted (e.g. `construct_path`, `resolve_child`, `RootGuard::new`, Windows `resolve_to_resource`) | Stale cross-refs only | Future corrective plan |
| low | `conformance/http3_qualification.toml` still points at pre-Plan-220 `core/src/server/http3/*.rs` paths | Stale cross-ref in qualification inventory (also flagged in Plan 293) | Future corrective plan |
| — | — | — | No medium+ findings |

## 11. Roadmap disposition

Milestone closed; no next dependency (docs-architecture refresh complete at
the 0.4.0 baseline). The two low findings are recorded above for future
plans, not as conditions on this closure.

## 12. Registry updates

- `plans/registry.md`: 301 rows active (roadmap table + implementation table
  + closure control point); roadmap table returns to closed on merge.
- `plans/subsystems/docs-architecture-roadmap.md`: §12 milestone 301 → closed.
