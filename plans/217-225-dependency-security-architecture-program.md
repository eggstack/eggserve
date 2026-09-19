# Plans 217–225 — Dependency, security, and authority-convergence program

## Objective

Complete the crate-ownership work started by Plans 211–216, address the current dependency advisory, and reduce cross-repository maintenance duplication without turning eggserve into a dependency hub for other products.

## Sequence

### Immediate, parallel-safe

**Plan 218 — Dependency security remediation and advisory automation**

Land first. Patch rustls floors/lockfiles and add scheduled advisory scanning. This does not depend on architecture work.

### Core authority convergence

**Plan 217 — Direct service/request type convergence**

Finish the service/request convergence left explicitly open by Plan 216, including compatibility H2 dispatch through the canonical service shape.

**Plan 219 — Static/confinement authority collapse**

Delete the second path/filesystem/static implementation from `eggserve-core` and make `eggserve-static` authoritative.

These two plans may be developed in parallel only if they avoid modifying the same compatibility facade modules. Prefer 217 before the final 219 facade cleanup when type ownership intersects.

### Optional transport and first-party consumers

**Plan 220 — H3 adapter extraction**

After canonical service types are stable, move the actual H3 adapter into `eggserve-h3`.

**Plan 221 — First-party frontend leaf-crate migration**

Migrate binary first, then Python, away from the compatibility core.

Plan 221 should consume the ownership established by 217/219 and preferably 220.

### Cross-repository consolidation

**Plan 222 — eggnet-tls server TLS consolidation**

Use neutral TLS substrate in eggserve/eggress; verify eggress optional mTLS.

**Plan 223 — outbound HTTP CONNECT consolidation**

Share only the H1 caller-owned-stream CONNECT wire primitive between eggfetch/eggress. Eggserve remains independent.

These do not need to block eggserve core closure.

### Evaluation

**Plan 224 — capability filesystem crate evaluation**

Only after Plan 219. GO/NO-GO gate; no automatic crate proliferation.
Closed NO-GO: `eggserve-static` remains the single confinement authority
(see `release/plan-224-capability-filesystem-evaluation.md`).

### Closure

**Plan 225 — compatibility-core facade closure**

Final proof that core is no longer an implementation authority.

## Global invariants

- Eggserve does not depend on eggfetch or eggress as products.
- Shared crates exist only for genuinely neutral behavior.
- `eggserve-primitives` stays transport/runtime neutral.
- `eggserve-server` stays free of static serving and H3/QUIC dependencies.
- `eggserve-static` owns static-file semantics and confinement (Plan 224 closed NO-GO; no capability-filesystem crate).
- `eggserve-h3` is optional and experimental.
- `eggnet-tls` remains product/transport neutral.
- First-party frontends should consume canonical leaf crates directly.
- Security-critical behavior has one implementation authority.
- Existing support tiers do not change as a side effect of refactoring.

## Completion definition

The program is complete when:
1. current dependency advisories are remediated and scheduled monitoring exists;
2. request/service/static/filesystem authority is single-source;
3. H3 implementation is outside compatibility core;
4. binary/Python exercise direct crates;
5. cross-repo TLS/CONNECT duplication has either been consolidated or explicitly retained with rationale;
6. filesystem crate extraction has a documented GO/NO-GO result;
7. Plan 225 full topology/security/package closure passes.
