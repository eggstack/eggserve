# Plan 225 — Compatibility-facade closure

## Decision

**CLOSED.** `eggserve-core` is proven to be a compatibility facade rather
than an implementation authority, closing the 217–224 architecture program.
No behavior changes follow from this plan; it is the proof/cleanup gate
after the preceding migrations.

Preconditions held: Plan 217 (service/request convergence), Plan 219
(static/confinement collapse), Plan 220 (H3 extraction), Plan 221
(first-party leaf migration), and Plan 218 (supply chain) were complete;
Plans 222–224 were closed as cross-repo/evaluation follow-ons with no
blocking ownership problem.

## Implementation inventory (Plan 225 §1)

Every production module in `crates/eggserve-core/src` (64 files) is
classified:

- **Compatibility re-export (facade):** `ops/mod.rs`, `policy.rs`,
  `runtime_limits.rs`, `tls.rs` (delegates QUIC assembly to `eggserve-h3`),
  `server/errors.rs`, `server/response_policy.rs`, `server/service.rs`,
  `server/config/http3.rs`, and all of `primitives/*.rs` except the two
  below — each carries `pub use eggserve_...` over the direct authority.
- **Compatibility adapter:** `primitives/interop.rs` (optional Plan 200
  `http-interop` loss-aware conversions, never in default builds),
  `primitives/canonical.rs` (inline `adapters` submodule delegating to
  `eggserve_server::adapters`), `server/tower.rs` (optional Plan 200
  Tower adapters), `response.rs` (`pub(crate)` runtime error helpers).
- **Unavoidable orchestration (documented, not a second authority):**
  `config.rs` (`ServeConfig`/`ServeState` bridges), `limits.rs`
  (static listing/extra-header budgets; shared checks delegate to
  `eggserve_server::runtime_limits`), `server/mod.rs` + `ServerBuilder`
  orchestration, `server/config.rs` + `config/runtime.rs`
  (`try_from_serve_config`, builder), `server/static_service.rs`
  (full `StaticService` composition over the static authority — no
  resolver of its own), `server/connection/*` (H1/H2 transport glue
  dispatching through the single `Service` contract and the shared
  `run_tunnel` future), `server/accept.rs`, `server/listener.rs`,
  `server/handle.rs`, `server/runtime.rs`, `server/lifecycle.rs`,
  `server/proxy.rs`, `server/http3.rs` (thin facade projecting
  core config/state into `eggserve-h3::accept_loop`).
- **Remaining implementation blocker:** none. No unclassified parser,
  filesystem resolver, protocol state machine, TLS builder, runtime
  limit table, or service model remains.

Two leftovers were removed by this plan:

1. `src/primitives/canonical/` — an unreferenced duplicate of the
   canonical response vocabulary (status/headers/body/normalization/
   adapters) sitting next to the `canonical.rs` facade. No `mod`
   declaration referenced it; it was dead code shipped in the package.
   Deleted; the topology gate rejects its return.
2. The direct `phf` dependency — a leftover from the deleted MIME
   implementation (Plan 219). The MIME table lives once in
   `eggserve-static`; core keeps no `phf` edge. `phf` remains in the
   resolved closure exactly once, transitively through `eggserve-static`.

## Dependency minimization (Plan 225 §2)

`cargo tree --prefix none` line counts (resolved package instances),
before → after:

| Closure | Before | After |
|---------|--------|-------|
| core default | 250 | 249 |
| bin default | 157 | 156 |
| server direct | 65 | 65 |
| static direct | 118 | 118 |
| core `http2,tls` | 257 | 256 |
| core `http3,tls` | 331 | 330 |

The −1 in each core/bin closure is the removed direct `phf` edge
(`phf v0.11.3` remains transitively via `eggserve-static`, the single
MIME authority). No other direct dependency could be removed: `bytes`,
`futures-util`, `http-body`, `http-body-util`, `httpdate`, `hyper`,
`hyper-util`, `thiserror`, and `tokio` are all exercised by the
documented transport glue and orchestration above; `http` /
`tower-service` / `tower-layer` are optional interop features, never in
default builds. No security-sensitive dependency enters or leaves any
closure; binary size was not gated (ownership clarity is the metric).

## First-party proof (Plan 225 §3)

`eggserve-bin` and `eggserve-python` name `eggserve-primitives`,
`eggserve-server`, `eggserve-static`, and `eggnet-tls` directly for every
neutral path (policy, observability, shared limits via the single
`SharedRuntimeValues` authority, static planning/capabilities, tunnel,
neutral TLS loading), enforced by the Plan 221 topology rules. The
narrow remaining compatibility uses are the documented extended
orchestration (full TLS/H2/H3 `Server`, full `StaticService`,
`ServeConfig`/listing budgets, handle lifecycle, `run_cli`), not second
implementations. Downstream consumers can build on the direct crates
alone: `crates/eggserve-server/examples/caller_owned.rs` drives the leaf
H1 server with no `eggserve_core` import, and the binary unit tests drive
leaf `Server` + leaf `StaticService`.

## Topology hardening (Plan 225 §4)

`scripts/check-crate-topology.py` gains `check_plan225_facade()`:

- no `primitives/canonical/` second implementation;
- no `phf` in core dependencies (including target-gated sections);
- a classified 64-module production inventory — unclassified new modules
  fail until explicitly classified per §1 (rollback rule: do not silently
  re-expand core);
- facade discipline: every `primitives/*.rs` file carries
  `pub use eggserve_...` except the documented Plan 200 `http-interop`
  adapters (`interop.rs`; `mod.rs` only declares modules).

Ownership markers and import direction are checked, not line counts.

## Documentation (Plan 225 §5)

`plans/ROADMAP.md` marks 225 closed;
`architecture/crate-topology.md` records the closure, the classified
remainder, and the new gate; `architecture/overview.md`,
`architecture/eggserve-core.md` (facade-corrected canonical row,
Plan 225 dependency table), `architecture/eggserve-bin.md`, and
`architecture/eggserve-static.md` drop the "until Plan 225" wording;
`README.md`, `AGENTS.md`, and the `eggserve-dev` skill mark core as the
0.1 compatibility facade; `docs/dependency-policy.md` scopes `phf` to
`eggserve-static` and `rustix` (`net`-only in core);
`docs/migration-guide.md` gains a "no migration required" 225 section
with the deprecation/removal strategy (removal needs a separate explicit
migration plan); `docs/downstream-app-server.md` records that the direct
crates are sufficient; `docs/public-api-boundary.md` records the
facade proof.

## Release evidence (Plan 225 §6)

Full routine matrix, all green locally before push (see CI run on the
merge commit for remote confirmation):

- `python3 scripts/verify-conformance-matrix.py`
- `python3 scripts/check-crate-topology.py` (with the new Plan 225 gate)
- `python3 scripts/check-python-release-metadata.py`
- `cargo fmt --all -- --check`
- `cargo +1.88 check --workspace --all-targets` (+ `http2,tls` / `http3,tls`)
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`
- `cargo test --workspace`
- excluded Python crate parse check + feature-matrix clippy/test lanes
  (`http2,tls` / `tls` / `http3,tls` for core and bin)
- `scripts/check-supply-chain.sh` (both lockfiles)
- `scripts/verify-cargo-packages.sh --mode all`
- `cargo test --doc -p eggserve-core`, examples check, both dist builds
- Python wheel suite (`scripts/test-python-wheel.sh`, via CI python job)

Incidental fix found during evidence collection:
`verify-cargo-packages.sh --mode all` failed on main before this plan
(the `eggserve-bin` stage did not rewrite the Plan 221 leaf path edges
to the local registry, so the staged bin manifest could not resolve
`eggnet-tls`). The bin rewrite rules now cover the leaf crates; the
failure was reproduced on the base commit and passes with the fix.

No support-tier change follows from this architecture work: H1 and
canonical `primitives` stay supported; `server`/H2/H3/tunnel/trailer/
adapter/listener/proxy/TLS-identity/async-Python stay experimental per
Plan 208.

## Core deletion question

This plan does not delete `eggserve-core`. Before 1.0, evaluate whether
keeping the compatibility umbrella is useful for ergonomics or a later
breaking release should deprecate it in favor of the direct crates. Any
removal requires a separate explicit migration plan with release notes
and migration guidance.
