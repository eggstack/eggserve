# Plan 286 — Embedding-contract publication and registry-only closure

## Status

**SOURCE PACKAGE SET DERIVED; registry publication waits for Plan 285 hosted CI.**

Plan 279 is source-closed. Per maintainer direction, its registry-only closure
and the pending Plan 277 candidate are consolidated here; they are not
intermediate blockers to source qualification.

## Purpose

Publish the qualified direct-H1 embedding policy-ownership contract and prove
it from fresh crates.io-only consumers before downstream projects are told the
new runtime boundary is available.

This plan performs no new runtime design.

## Release-version authority

At execution time:

1. query crates.io for every affected EggServe package;
2. read Plan 285's measured compatibility decision;
3. select the next unused compatible release.

Rules:

- if Plan 285 proves the API is additive/source-compatible under the current
  0.2 contract, use the next unused 0.2.x patch;
- if Plan 285 records an incompatible public change, use 0.3.0;
- never assume a version number from this planning document;
- never overwrite or mutate a previously qualified/published candidate.

Pre-release crates.io baseline queried 2026-09-24:

| Package | Latest published |
| --- | --- |
| `eggserve-server` | 0.2.1 |
| `eggserve-primitives` | 0.2.0 |
| `eggserve-core` | 0.2.2 |
| `eggserve-static` | 0.2.0 |
| `eggserve-h3` | 0.2.0 |
| `eggserve-bin` | 0.2.0 |
| `eggnet-tls` | 0.2.0 |

Selected candidate graph after Plan 285's 0.3.0 direct-server decision:

| Package | Candidate | Reason |
| --- | --- | --- |
| `eggserve-primitives` | 0.2.1 | Additive absolute-form target vocabulary required by server source |
| `eggserve-server` | 0.3.0 | Incompatible public `RuntimeConfig` and admission-accessor changes |
| `eggserve-static` | 0.3.0 | Must use the same server `Service` and primitives authorities |
| `eggserve-h3` | 0.3.0 | Must use the same server `Service` and primitives authorities |
| `eggserve-core` | 0.3.0 | Compatibility composition over the new server/static/H3 authorities |
| `eggserve-bin` | 0.2.1 | Candidate CLI graph now depends on the new core/server line; CLI API is not major-bumped |

`eggnet-tls` and Python's distribution version are not part of the Cargo
publish set. Python's excluded Rust manifest and lockfile are updated so the
local wheel build resolves the new crate graph.

## Track A — Derive the minimal publish set

Derive the changed packages from actual source/API/dependency changes.

Expected likely authorities:

- `eggserve-primitives` — request-target form and absolute metadata;
- `eggserve-server` — ownership/config/rejection/tunnel runtime work;
- `eggserve-static` and `eggserve-h3` — dependency identity follows server 0.3;
- `eggserve-core` — compatibility composition and tightened internal bounds;
- `eggserve-bin` — published CLI dependency graph follows core/server 0.3.

Do not republish `eggnet-tls` or the Python distribution merely for workspace
version aesthetics. Static/H3 are included because their public integration
types depend on the changed direct-server authority.

For every changed package, tighten dependency lower bounds enough that the
published source cannot resolve against an older sibling missing required API.

Publish in this dependency order, derived from the candidate manifests:

```text
eggserve-primitives 0.2.1
  -> eggserve-server 0.3.0
      -> eggserve-static 0.3.0
      -> eggserve-h3 0.3.0
          -> eggserve-core 0.3.0
              -> eggserve-bin 0.2.1
```

## Track B — Package and supply-chain qualification

Before publishing, run the exact release candidate through:

```bash
bash scripts/verify-cargo-packages.sh --mode all
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
```

Run `cargo publish --locked --dry-run` for every changed package in
dependency order when registry dependency visibility permits.

Use the repository's staged local-registry mechanism for dependent packages
that cannot dry-run against crates.io before their prerequisite is published.

Inspect packaged source to ensure:

- all new public modules/types are included;
- direct embedding fixtures/tests required by package policy are present where
  expected;
- no path/git/patch dependency leaks into publish metadata;
- no core/static/PHF dependency enters direct server unexpectedly.

## Track C — Publish dependency order

Publish manually in the derived order.

For each package:

- wait for crates.io sparse-index/API visibility;
- record publication timestamp;
- record crates.io/index checksum;
- run `cargo info` or fresh resolution;
- stop immediately if registry metadata differs from the qualified candidate.

Do not publish a dependent package until its exact prerequisite version is
resolvable.

## Track D — Registry-only default consumer

Create a fresh consumer outside the workspace with no path/git/patch
overrides.

Use exact published versions.

Prove the ordinary direct server still has hardened defaults:

- EggServe owns handler/body/idle/write deadlines;
- EggServe owns global body and target ceilings;
- EggServe owns service/tunnel admission;
- default runtime error representation is unchanged;
- parser/header limits remain active;
- typed shutdown/control remains functional.

This fixture prevents an embedding-oriented release from silently weakening
ordinary users.

## Track E — Registry-only advanced embedding consumer

Create a fresh generic policy-owning host using only crates.io artifacts.

Required shape:

```text
Tokio TCP listener
 -> local Rustls handshake
 -> ALPN http/1.1
 -> server TlsStream
 -> published eggserve-server direct H1 policy API
 -> canonical Service
```

Enable all supported advanced ownership modes.

Required proof:

- external handler/body/idle/write ownership;
- external global body ceiling with a service-selected body bound;
- external semantic target ceiling while mandatory parser validation remains;
- external service admission;
- external tunnel admission;
- custom typed runtime-rejection presentation;
- caller shutdown/drain;
- connection total timeout enabled and explicitly disabled cases;
- upgrade/CONNECT tunnel behavior through the retained Plan-284 design.

The consumer must use no EggServe core/static/TLS compatibility orchestration.

## Track F — Registry-only Tower consumer

Create a second fresh consumer using:

```toml
eggserve-server = { version = "=<published>", default-features = false, features = ["tower"] }
```

Prove:

- Axum/Tower request/response composition;
- streaming request body;
- streaming response;
- trailers;
- duplicate headers;
- lifecycle cancellation;
- external policy/admission ownership;
- runtime rejection presentation where applicable.

Run `cargo tree -e no-dev` and prove no
`eggserve-core` / `eggserve-static` / PHF ancestry.

## Track G — Compatibility/core consumer

If `eggserve-core` is in the publish set, or if server changes could affect
its facade contract, build a registry-only compatibility consumer proving:

- historical core import paths still compile;
- static service remains bounded/default-safe;
- core Tower forwarding still works when enabled;
- no new advanced embedding mode becomes default accidentally.

Do not make core mandatory for direct embedding.

## Track H — Exact published-artifact measurements

For the native advanced and Tower consumers record:

- resolved versions;
- checksums;
- `cargo tree -e no-dev`;
- `cargo metadata --format-version 1`;
- package count;
- release executable size under the standard qualification profile.

If Plan 284 retained a tunnel optimization, optionally re-run one
representative registry-only tunnel benchmark as a smoke/provenance check; do
not substitute it for Plan 284's same-machine A/B evidence.

## Track I — Closure evidence

Create:

`release/plan-286-embedding-contract-publication-closure.md`

Record:

- Plan-285 proof-bearing SHA and hosted CI run;
- Plan-284 KEEP/NO-GO decision;
- selected release version and compatibility rationale;
- changed publish set and order;
- publication timestamps/checksums;
- dry-run/staged-registry results;
- default registry-only consumer result;
- advanced TLS-H1 registry-only consumer result;
- Tower registry-only consumer result;
- core compatibility result if applicable;
- dependency graph/package measurements;
- known residuals/non-goals.

Update `plans/ROADMAP.md` only after exact published artifacts pass.

## Downstream handoff wording

Use precise language similar to:

> Published EggServe direct H1 runtime now supports explicit external ownership
> for selected application deadlines/semantic ceilings and service/tunnel
> admission while retaining hardened EggServe-owned defaults and mandatory
> parser/framing protections. Registry-only caller-owned TLS and Tower
> consumers prove the embedding contract.

Do not claim EggServe itself provides the external policy.

## Hosted CI

The release candidate must have green hosted CI before publication.

The final evidence/roadmap commit should also receive hosted CI. Record its
state separately from the proof-bearing source candidate.

## Acceptance criteria

- [ ] Plan 285 qualification and hosted CI are closed.
- [ ] release version is selected from live registry + measured compatibility.
- [ ] minimal publish set is derived.
- [ ] sibling dependency lower bounds cannot resolve incompatible older APIs.
- [ ] package/supply-chain gates pass.
- [ ] packages publish in dependency order.
- [ ] timestamps/checksums are retained.
- [ ] registry-only default consumer proves hardened defaults unchanged.
- [ ] registry-only advanced TLS-H1 consumer proves external ownership.
- [ ] registry-only Tower consumer proves direct framework composition.
- [ ] direct dependency graph excludes core/static/PHF.
- [ ] core compatibility is proven where relevant.
- [ ] no path/git/patch override appears in final proof.
- [ ] release closure record and roadmap are reconciled.
- [ ] downstream unblock is not claimed before all exact-artifact checks pass.

## Non-goals

- No new runtime behavior.
- No downstream repository dependency bump.
- No automatic publication.
- No PyPI release unless a separately changed Python package requires it.
- No H2/H3 support promotion.
