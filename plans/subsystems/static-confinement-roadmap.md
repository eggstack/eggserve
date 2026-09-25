# Static Confinement Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#2` (sole static/path/filesystem authority)
- `plans/001-terminology-and-domain-model.md` (`ConfinedPath`, `SecureRoot`, `StaticPolicy.symlinks`)
- `plans/002-long-term-roadmap.md` (phase 1 + authority convergence)

Related ADRs:

- `architecture/adr-002-windows-handle-relative-filesystem.md` (accepted; stays in place, linked not moved)

## 1. Purpose and ownership boundary

Owns all static/path/filesystem decisions: request-target parsing, `ConfinedPath` validation, `SecureRoot`/`PinnedRoot` confinement (Unix descriptor-relative `statat`+`openat`, Windows handle-relative), capability bridge, MIME selection, conditional/range/ETag planning, and `StaticService` request planning + rendering.

Consumes: canonical `primitives` types, `server` admission/connection context. Must not own: H1 wire runtime, TLS identity, QUIC transport.

Checked by `scripts/check-crate-topology.py` (`eggserve-static` sole authority; `src/fs`, `src/path`, `src/mime.rs` deleted from core; Plan 224 NO-GO: no capability-filesystem crate).

## 2. Work classification

### Invariants

- No serving outside the configured root under safe defaults.
- Dual `DotfilePolicy` types (parsing + serving) must agree for dotfiles to be served.
- `StaticPolicy` field is `symlinks`, not `follow_symlinks`; directory listing stays opt-in.

### Capabilities

- Confined `GET`/`HEAD` file serving with correct `Content-Length`, MIME, `Last-Modified`/ETag, index vs listing behavior.
- Conditional/range planning with HEAD parity and body-forbidden normalization.

### Infrastructure

- Descriptor/handle-relative resolver, planner, MIME tables, `StaticService::canonical_response()` adapter.
- Authority conformance fixture (`crates/eggserve-core/tests/static_authority_conformance.rs`).

### Polish

- Listing-buffer behavior, HTML escaping/headers, allocation fixed-cost cleanup (Plans 229/236).

## 3. Non-goals

- No uploads/writes, routing, middleware, templating, auth framework, reverse proxying (`docs/non-goals.md`).
- No second resolver or pathname check-then-open fallback.
- No capability-filesystem crate (Plan 224 NO-GO, immutable).

## 4. Current state

Closed. `eggserve-static` is the single implementation authority; `eggserve-core` keeps re-export facades only. Windows handle-relative confinement qualified functional-only (two NTFS open-descendant root-rename cases skipped); Unix descriptor-relative traversal is the safe-default path.

## 5. Target architecture

No change: preserve single authority, facade discipline, and the pure-planner → `canonical_response()` adapter shape.

## 6. Dependency graph

```text
path confinement + SecureRoot (hard, closed)
    |
    +--> planner/MIME/StaticService (hard, closed)
             |
             `--> authority convergence + facade closure (closed: 219/224/225/245)
```

## 7. Milestones

### Milestone 1 — Path confinement and SecureRoot

Class: invariant

Objective: deny traversal/symlink escape at library level under safe defaults.

Dependencies: none (foundation).

Deliverable boundary: 6-stage pipeline, 17 `PathRejection` variants, fuzz targets + corpora.

User or operator value: root escape is structurally denied, not policy-advised.

Exit conditions: traversal/double-encoding/absolute/NUL/dotfile/symlink regressions green; fuzzer invariants hold.

Deferred work: none.

### Milestone 2 — Static planning and service authority

Class: capability

Objective: single `StaticService` plans/renders all static responses; core keeps a wrapper.

Dependencies: Milestone 1 (hard).

Deliverable boundary: conditional/range/ETag planner, HEAD parity, `normalize_response` convergence.

User or operator value: correct, predictable static semantics over any transport.

Exit conditions: authority conformance fixture + H1 parity suite green; topology gate owns the boundary.

Deferred work: none.

## 8. Cross-cutting requirements

Storage: read-only serving; no migration. Protocol: origin-form only; absolute-form rejected by static (Plan 278 seam stays H1-embedding-only). Security: `docs/threat-model.md` layers 1–3. Concurrency: file-stream semaphore + bounded chunk reads (128 KiB default, Plans 232–233). Observability: `OpsContext` events, sanitized paths. Perf: per-trial retained evidence, never single best numbers.

## 9. Verification strategy

Rust unit/integration suites + `conformance_matrix.toml` replay + 11 fuzz targets (corpus replay in `cargo test`) + topology gate + Windows adversarial suites (manual qualification lane).

## 10. Risks and decision points

None open. Any future confinement-boundary change requires an ADR.

## 11. Completion definition

Authority collapse proven by fixture + gate, facades classified, no second implementation, evidence in `release/plan-225-compatibility-facade-closure.md` and `release/plan-248-maintainability-convergence-closure.md`. Met.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | legacy `plans/002-*`, `007-*`, `017-*`, `062-*`–`065-*` | `release/plan-034-*` lineage; `plan-225-*` facade proof | — |
| 2 | closed | legacy `plans/081-*`, `219-*`, `245-*` | `release/plan-225-compatibility-facade-closure.md` | — |
