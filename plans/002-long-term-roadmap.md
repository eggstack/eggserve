# EggServe Long-Term Roadmap

Status: execution roadmap for `plans/000-long-term-specification.md` (Plan 292).

Terminology: `plans/001-terminology-and-domain-model.md`.

Full milestone history lives in `plans/ROADMAP.md` and the legacy flat plans; this file orders the standing work without copying that history. Each phase MUST leave the repository in a coherent state with implementation plans, tests, documentation, and closure evidence before dependents proceed. The roadmap is dependency-ordered, not calendar-ordered.

## Standing phases (all closed unless noted)

1. **Static-serving foundation** — path confinement, filesystem policy, static MVP, limits/hardening, CLI + wheel handoff, fuzz/CI/TLS/Python API/library stabilization. History: legacy Plans 000–060 series + `ROADMAP.md` M0–M9.
2. **Authority convergence** — direct H1 runtime parity (215–217), static-authority collapse (219), H3 extraction (220), frontend leaf migration (221), cross-repo TLS/CONNECT (222–223), capfs NO-GO (224), facade closure (225), release corrective (226). Evidence: `release/plan-225-*`, `plan-226-*`.
3. **Maintainability convergence** — shutdown/drain + H1/static/Python/orphan cleanup (242–248) with H1-authority lifetime corrective (249–250) and interop-fidelity + async-lifetime maintenance (251–258). Evidence: `release/plan-248-*`, `plan-250-*`, `plan-256-*`, `plan-258-*`.
4. **Distribution expansion** — wheel matrix/ABI/platform program (263–269). Evidence: `release/plan-264-266-*`, `plan-267-*`, `plan-269-*`.
5. **Direct embedding contract** — supervisory lifecycle + total-lifetime opt-out (270–271), downstream qualification + patches (272–275), adapter extraction (276–277), absolute-form seam (278–279), policy/admission/connection-policy/rejection/tunnel ownership + qualification + publication (280–286), parser/header/metadata follow-up (288–291). Evidence: `release/plan-272-*`, `plan-275-*`, `plan-277-*` through `plan-291-*`.
6. **Planning-convention migration (ACTIVE)** — Plan 292 (this program): stand up the CodeGG-style hierarchy, seed canonical docs + registry + exemplar roadmaps, repoint agent docs. No product change.

## Dependency notes

- New transport/tier promotion work is **operationally** gated on independent-client + adversarial + platform evidence (Plans 191–195 precedent); code existence never promotes a tier.
- Publication milestones are **operationally** gated on registry-only consumer proof + exact-SHA hosted CI (Plans 286/291 precedent).
- Cross-repo TLS/CONNECT work keeps `eggnet-tls` published from this workspace as a versioned package, never a git dependency.

## Exit criteria (roadmap-level)

- Every active subsystem roadmap reaches `closed` with immutable closure records.
- `registry.md` shows no `active`/`blocked`/`closing` rows except explicitly accepted follow-ups.
- Canonical docs change only via explicit architecture decisions with ADR links.
