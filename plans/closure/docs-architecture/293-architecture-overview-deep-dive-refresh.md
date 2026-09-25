# Docs Architecture Milestone 293 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/docs-architecture/293-architecture-overview-deep-dive-refresh.md`

Source subsystem roadmap:

- `plans/subsystems/docs-architecture-roadmap.md#7`

Repository baseline reviewed: `main` post-Plan-292 (closed 288–291 code state)

Implementation commits or pull requests:

- (this commit) — docs-only refresh of `architecture/overview.md` + 19 deep dives, plus Plan 293 registration

## 1. Executive finding

The milestone's polish boundary is complete. `architecture/overview.md`
remains the bird's-eye index (2–4 sentence overviews per discrete
module/tool/capability with links to owning deep dives); all 23 deep dives
were walked once via six parallel subagents against current code, and every
hard-stale claim found was corrected. No behavior, API, dependency, or tier
change. No open findings require a corrective code plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Overview crate sections verified | subagent groups 1–3 reports; `Cargo.toml` versions checked | pass | Fixed server version `0.3.0`→`0.3.1` line, `http-interop`/`tower` flag rows, server-deps wording |
| Overview tool map verified | subagent group 6 counts: 11 fuzz targets, 51/55(47)/17 corpora, 19 scripts, 26 arch files, 19 cross-protocol tests, 3 CI jobs | pass | Fixed soak `fixtures/`, Plan 109 `binary-size.md` pointer |
| Every deep dive walked once | 6 subagent reports covering all 23 deep dives + 2 ADRs referenced | pass | Reports retained in session; findings below |
| Stale claims corrected | ~40 targeted doc edits across 20 files | pass | Listed in §3; precision-only notes skipped |
| Links resolve | link-resolution check over `architecture/*.md` | pass | `ALL LINKS RESOLVE`; fixed 6 `release/`→`../release/` + 1 `architecture/overview.md`→`overview.md` + H3 `../` paths + 1 plan-256 filename |
| Invariants preserved | topology + conformance gates; docs-only diff | pass | No code touched |
| Tier labels consistent | H1 + primitives supported; remainder experimental kept | pass | Fixed runtime.md stale `Experimental` banner |
| Registry + roadmap updated | `plans/registry.md`, `docs-architecture-roadmap.md` §12 | pass | Done in this commit |

## 3. Production implementation evidence

Docs-only: no production-code changes. Doc corrections applied:

- `overview.md`: version line (`server 0.3.1`, `primitives 0.2.1`), `http-interop`/`tower` feature rows, server-deps wording, soak-fixture + benchmark-index fixes.
- `runtime.md`: scoped experimental banner, removed legacy `Server::start()` non-Clone line, fixed `server/runtime.rs`→crate paths, `H1ConnectionPolicy` file-stream inclusion, direct-only `serve_http1_*` facade, `0.3.0/0.3.1` line label.
- `eggserve-server.md`: `H1ConnectionPolicy` chunk-size fix, node-count removal, fixture-glob fix.
- `eggserve-primitives.md`, `path-confinement.md`: Plan 278 absolute-form wording; `TooLong` active (only `AbsolutePath` reserved).
- `response-planning.md`: `HeaderMapPlan` privacy, `MultipleRanges` variant, ETag pre-epoch note, link fix.
- `tls.md`: TCP-vs-QUIC assembly, bin path + feature gate, `pub(crate)` delegate, ALPN empty-list semantics.
- `eggnet-tls.md`: dev-deps `rcgen` addition.
- `http2.md`: suite path, Extended-CONNECT scoping.
- `http3.md`: Extended-CONNECT support correction (generic `:protocol` still rejected).
- `eggserve-h3.md`: `pub(crate)` visibility, `../` link paths.
- `eggserve-core.md`: ops-facade row, `http1.rs` placeholder row, `into_parts` attribution to direct handle, missing `server/` modules, full 8-example list, 3 link fixes.
- `eggserve-bin.md`: `tls` gate note, 1 link fix.
- `eggserve-python.md`: `registration.rs` in structure diagram.
- `filesystem-confinement.md`: streaming-path filename fix.
- `configuration.md`, `primitives-api.md`: `http3` envelope ownership, `http1.rs` placeholder, `SecureRoot` import location, presenter visibility, method visibility, 1 link fix.
- `error-taxonomy.md`: presenter signature.
- `testing-and-conformance.md`: `verify.sh full` vs wheel-harness split, server-examples dir, benchmark index, `fuzz_directory_buffer` Windows-only note.
- `security-model.md`: normative invariant cross-quote, 5-stage label, framing-enforcement note, platform-qualification tone, 2+2 unsafe boundaries with crate paths, concurrent-mutation scope fix.

## 4. Verification executed

### Commands run

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo fmt --all -- --check
# + architecture/ link-resolution check (ALL LINKS RESOLVE)
# + git status (docs + plans only, no code)
```

### Results

- Conformance matrix: pass (51 entries, 55 scenarios/47 routine, 17 H3).
- Topology gate: pass (Plans 211–249 + 253 rules).
- `cargo fmt --check`: pass.
- Links: all resolve.
- `verify.sh fast`/`full` not run: docs-only change with no code impact; plan §11 requires only the three gates above.

## 5. Invariant review

- Safe defaults / no-serving-outside-root / static authority / H1 authority / QUIC-behind-`http3`: untouched (docs-only); security-model wording aligned toward normative `docs/threat-model.md`, not away.
- `OpsContext` logging, one-shot bodies, framing ownership: claims re-verified, unchanged.
- Overview remains the index; every link resolves.

## 6. Failure and recovery review

Docs-only: no runtime failure modes. Partial-completion risk (unwalked dive)
did not materialize — all 23 walked. Code/doc contradictions found were all
doc-stale (code wins); none required stopping per §14 stop conditions.

## 7. Migration and compatibility review

No compatibility effect. Version strings kept at published values
(`eggserve-server 0.3.1`, `static`/`h3`/`core 0.3.0`, `primitives 0.2.1`,
`bin 0.2.1`, wheel `0.2.3`, PyO3 `0.29.2`). One observation for awareness
(no doc change): `crates/eggserve-python/Cargo.toml` pins
`eggserve-bin 0.2.0` while bin is `0.2.1` — flagged, not altered (out of
docs scope).

## 8. Security review

No confinement/policy change. `security-model.md` deltas are alignment-only
toward `docs/threat-model.md` (normative). No new attacker capability, no
`non-goals.md` crossing.

## 9. Documentation and operations

Updated: `architecture/overview.md` + 19 deep dives (list in §3),
`plans/subsystems/docs-architecture-roadmap.md` (§12),
`plans/registry.md` (293 closed), this closure record. No `docs/`
normative edits. No guard-script changes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `conformance/http3_qualification.toml` still points at pre-Plan-220 `core/src/server/http3/*.rs` paths | Stale cross-ref in qualification inventory | Future corrective plan (out of docs-architecture scope) |
| low | Python-crate `eggserve-bin 0.2.0` pin vs published `0.2.1` | Awareness only; wheel builds resolve compatibly | Owner to confirm on next wheel-plan |
| — | — | — | No medium+ findings |

## 11. Roadmap disposition

Milestone closed; no next dependency (single-milestone roadmap). The two low
findings are recorded above for future plans, not as conditions on this
closure.

## 12. Registry updates

- `plans/registry.md`: 293 rows moved to closed (roadmap table + implementation table + closure control point).
- `plans/subsystems/docs-architecture-roadmap.md`: §12 milestone 293 → closed with plan/closure links.
