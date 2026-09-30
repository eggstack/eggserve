# Milestone 302 Closure — Agent/Skill/Docs Staleness Sweep (0.4.0 line)

Status: closed (docs-only; no behavior, API, dependency, or tier change)

Implementation plan:
`plans/implementation/planning-governance/302-agent-skill-docs-staleness-sweep-040.md`

## Requirement-to-evidence matrix

| Plan §3 finding | Correction | Evidence |
|---|---|---|
| Skill ends at 288–291, 0.3.x as current | 286 versions fixed (`server 0.3.0`); 293–302 paragraph added (0.4.0 line, 300 active, 302 sweep) | `SKILL.md` plan-history section; manifests `crates/*/Cargo.toml` |
| `AGENTS.md` stale versions + `fast` scope | `server/static/h3/core 0.4.0` etc.; `fast` keeps +1.89 interop/tower, skips workspace MSRV matrix/wheel self-tests/TLS-bin | `AGENTS.md` (prior session commit, carried in this diff) |
| `README.md` `"0.3"` snippets | `"0.4"` ×2; `0.4.0` ships 288–291 APIs (introduced `0.3.1`) | `crates/eggserve-core,eggserve-server/Cargo.toml:3` |
| `docs/http-interop.md` `"0.3"` snippets | `"0.4"` ×3 | same manifests |
| `docs/release-contract.md` metadata + line | 0.4.0-line metadata; `main` on `0.4.x`, Plan 171 transition shipped in `0.2.0` | manifests + `plans/registry.md:49` |
| `docs/tls.md` banner `0.2.1` ×2 | `0.2.2` | `crates/eggserve-bin/Cargo.toml:3`; binary prints `env!("CARGO_PKG_VERSION")` (`src/lib.rs:120`) |
| `docs/migration-guide.md` "next `0.2.0`" | "(historical)" | Plan 226 shipped the transition |
| `architecture/overview.md` "301 active" | "closed" | `plans/registry.md:35,48` |
| `architecture/eggserve-core.md` "now at `0.3.0`" | sentence removed; "now at `0.4.0`" stands | `crates/eggserve-core/Cargo.toml:3` |
| `architecture/structured-logging.md` example `0.3.0` | `0.2.2` | `crates/eggserve-bin/Cargo.toml:3` |
| `architecture/eggserve-bin.md` label | 0.3.0-line evidence (Plan 286); 0.4.0 is Plan 300 active | `release/plan-286-embedding-contract-publication-closure.md` published-set table |
| `plans/registry.md` "continue from `293`" | "continue from `302`" | milestones 293–301 taken |
| docs-architecture roadmap 301 "active" ×2 | "closed" + current-state note | registry 301 rows |

## Verification outcomes

```bash
python3 scripts/verify-conformance-matrix.py  # pass (51 matrix + 55 app-server + 17 H3 entries)
python3 scripts/check-crate-topology.py       # pass (Plan 211–253 rules)
cargo fmt --all -- --check                    # pass
git diff --check                              # pass (no whitespace errors)
```

`verify.sh fast`/`full` not run: docs-only change with no code impact (per
301 closure precedent).

## Invariants preserved

Topology + conformance gates; docs-only diff (no production source touched).
No legacy flat plan or `release/` record modified. Safe defaults,
confinement, crate authority, and tier labels untouched.

## Open findings

None. Skill CI sequence, layout, pins, and referenced paths verified current
by subagent audit; left unchanged.

## Roadmap disposition

Planning-governance roadmap returns to closed (Milestone 302 closed);
registry next-number advances to `302` (302 taken by this sweep; next new
milestone is `303`).
