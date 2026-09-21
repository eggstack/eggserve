# Plan 259 — Architecture overview refresh (bird's-eye + deep-dive index)

## Purpose

Refresh `architecture/overview.md` so it reads as a discrete, bird's-eye
overview of every module, tool, and capability, and as the canonical index
into per-component deep dives in `architecture/`.

This is a docs-only maintenance pass. It follows the authority split
(Plans 211–225), the maintainability convergence (Plans 243–250), and the
post-convergence maintenance campaign (Plans 251–258) without changing any
API, behavior, ownership, or support tier.

Planning baseline:

```text
a755ad1 docs: compress AGENTS.md to high-signal agent guide
```

## Constraints

- Docs only. No Rust/Python source, test, script, config, or dependency change.
- No public API addition, removal, rename, or signature change.
- No crate ownership, topology, or support-tier change (H1 + canonical
  `primitives` supported; `server`/H2/H3/tunnel/trailer/adapter/listener/
  proxy/TLS-identity/async-Python remain experimental).
- No rewrite of historical `plans/` or `release/` evidence.
- `plans/` + `ROADMAP.md` stay change-trace records, not normative API docs.
  Normative user contracts stay in `docs/`.
- Keep `0.2.0` as the intentional pre-1.0 line (Plan 226); never frame it as
  `0.1.x`.
- Keep the direct `rustls` `0.23.45` caret floor unmentioned-or-preserved;
  this plan does not touch manifests.

## Track A — Crate overviews (discrete, one paragraph each)

For each of the 8 crates, `overview.md` gives a 2–4 sentence discrete
overview plus its deep-dive link, key modules/types, dependency direction,
and stability tier:

1. `eggnet-tls` (neutral rustls identity/trust/client-auth/reload; leaf, no
   internal deps) → `architecture/eggnet-tls.md`.
2. `eggserve-primitives` (canonical transport-neutral model; leaf) →
   `architecture/eggserve-primitives.md` + `architecture/primitives-api.md`.
3. `eggserve-server` (single mature H1 connection runtime + single `Service`
   contract; depends on primitives only; H1-only, inert `http2`/`tls`
   compat names) → `architecture/eggserve-server.md` +
   `architecture/runtime.md`.
4. `eggserve-static` (SOLE static/path/filesystem/MIME/planning authority;
   consumes primitives + server; Plan 224 NO-GO recorded) →
   `architecture/eggserve-static.md` + path/filesystem/policy/planning pages.
5. `eggserve-h3` (experimental H3/QUIC adapter; sole QUIC dep owner;
   downward-only on primitives/server/`eggnet-tls`) →
   `architecture/eggserve-h3.md` + `architecture/http3.md`.
6. `eggserve-core` (compatibility/composition umbrella; facades only, no
   second implementation; H2/TLS/proxy/listener glue; Plan 225 closure) →
   `architecture/eggserve-core.md`.
7. `eggserve-bin` (static-only CLI; `main.rs` shim, real logic in
   `lib.rs`/`args.rs`; neutral paths name leaves directly per Plan 221) →
   `architecture/eggserve-bin.md`.
8. `eggserve-python` (workspace-excluded maturin/PyO3 wheel; `server`
   facade, `lowlevel` substrate + experimental H1-only async, `subprocess`
   helpers) → `architecture/eggserve-python.md`.

Dependency direction stays downward-only; the exact edges remain checked by
`scripts/check-crate-topology.py` (see `architecture/crate-topology.md`).

## Track B — Capability overviews (discrete, link each to a deep dive)

Give each capability 2–3 sentences plus its deep-dive link:

- Static serving + path/filesystem confinement + policy + response planning
  → `path-confinement.md`, `filesystem-confinement.md`, `policy-system.md`,
  `response-planning.md`, `security-model.md`.
- H1 runtime + `Service` contract + tunnel acceptance (inbound-only,
  Plans 199/216/217) → `runtime.md`.
- TLS identity (neutral) vs TLS transport (consumers) → `tls.md`,
  `eggnet-tls.md`.
- H2 / H3 experimental boundaries → `http2.md`, `http3.md`, `eggserve-h3.md`.
- Python facade / `lowlevel` / async substrate → `eggserve-python.md`.
- Trusted-proxy + PROXY protocol, listener ownership, body policy, error
  taxonomy, config ownership, structured logging (`OpsContext` boundary) →
  `configuration.md`, `error-taxonomy.md`, `structured-logging.md`,
  `runtime.md`.

Correct known index drift while here (no deep-dive rewrites):

- `crate-topology.md` blurb covers Plans 211–253 (not 211–224).
- Runtime row carries the Plan 249 qualifier (compatibility `Auto`
  classifies before any Hyper service exists; core executes H2 only).
- Ops row names `eggserve-server::ops` authority with the core facade
  (not core-owned).
- No deep-dive file is rewritten by this plan; residual staleness found
  during review (e.g. `tls.md` re-export target, `response-planning.md`
  `FileRange` literal, `error-taxonomy.md` locations/counts,
  `security-model.md` diagram label, `structured-logging.md` version
  example) is recorded as follow-up, not fixed here.

## Track C — Tool overviews (discrete, link to owning doc)

Summarize each tool family in `overview.md` with a 1-line-per-tool table
and point at the owning deep dive (`testing-and-conformance.md`,
`crate-topology.md`, `examples/README.md`, `docs/`):

- `scripts/verify.sh` tiers (`fast`/`full`/`deep`) + gate scripts
  (`verify-conformance-matrix.py`, `check-crate-topology.py`,
  `check-python-release-metadata.py`) + wheel/release scripts +
  `qualify-http2.sh` / `qualify-http3.sh` (manual).
- `conformance/` corpora (H1 matrix, body corpora, Plan 207
  cross-protocol inventory, H3 qualification).
- `fuzz/` 11 targets + seed corpora (property: no panic, no traversal,
  no double-decode).
- `benchmarks/` evidence policy (profiles + evidence files, never CI gates).
- `tests/` repo-level interop/soak shells.
- `examples/` canonical Python + Rust demos (indexed by
  `examples/README.md`).
- `docs/` normative contracts vs `plans/`+`release/` trace records.

## Verification

- Every `architecture/*.md` link in `overview.md` resolves on disk
  (26-file index check; no dangling links).
- `python3 scripts/check-crate-topology.py` passes (graph unchanged).
- `python3 scripts/verify-conformance-matrix.py` passes (corpora untouched).
- `cargo fmt --all -- --check` unaffected (markdown only); no Rust changed.
- `git status` shows only `plans/259-*.md` + `architecture/overview.md`.

## Follow-ups (out of scope, not fixed here)

- `tls.md` vs `eggserve-bin.md` re-export-target contradiction.
- `response-planning.md` `FileRange` public-field literal vs private
  fields + `try_new`/`new` + accessors; module-location wording.
- `error-taxonomy.md` ownership locations + row-count gaps.
- `security-model.md` "eggserve-core (policy layer)" diagram label.
- `structured-logging.md` ops-authority lede + `0.1.0` example version.
- Per-deep-dive Plan 243–258 coverage gaps (each file owns its own update).
