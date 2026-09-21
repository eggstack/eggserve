# Plan 260 — Systematic architecture deep-dive refresh

## Purpose

Fix the stale and omission drift found during the Plan 259 review across
`architecture/` deep dives, so each file is a trustworthy focused entry
point for review. Docs only.

Planning baseline:

```text
2ae733a docs: refresh architecture overview as bird's-eye index (259)
```

## Constraints

- Docs only (`architecture/` + this plan). No Rust/Python source, test,
  script, config, or dependency change.
- No public API, ownership, topology, or support-tier change (H1 +
  canonical `primitives` supported; `server`/H2/H3/tunnel/trailer/adapter/
  listener/proxy/TLS-identity/async-Python remain experimental).
- No rewrite of `plans/` or `release/` evidence. Cite them, don't duplicate.
- Every factual claim must be verified against the code cited (Read/Grep),
  not reconstructed from memory.
- Keep the direct `rustls` `0.23.45` caret floor and `0.2.0` pre-1.0 line
  statements intact.
- Keep all `architecture/overview.md` index links resolving.

## Track A — Crate authority docs

Files: `crate-topology.md`, `eggserve-server.md`, `eggserve-static.md`,
`eggserve-core.md`, `eggserve-h3.md`, `eggserve-bin.md`.

- `crate-topology.md`: extend the Plans 243–247 + 249 + 253 coverage to
  name 248 (closure), 250 (lifetime corrective), 251–252 + 254–258
  (maintenance/interop/async lifetimes) at index granularity. No new rules.
- `eggserve-server.md`: add the missing authority notes — 243 (durable
  shutdown/task drain), 244/249 (single-H1-authority corrective, `Auto`
  classifies before Hyper, core H2-only), 253 (overlap ledger pointer).
- `eggserve-static.md`: add Plan 245 (direct `StaticService`
  request-planning/rendering ownership; core wrapper).
- `eggserve-core.md`: add 243–258 index notes (H1 delegation,
  static-wrapper status, overlap ledger, typing/orphan cleanup, async
  parity); qualify `server/connection/driver.rs` as H2-only per Plan 249.
- `eggserve-h3.md`: add Plan 253 pointer (shared-kernel vs adapter split,
  Alt-Svc ownership in the ledger).
- `eggserve-bin.md`: add Plan 249 delegation note to the accept-loop
  section (H1 delegates to the direct driver).

## Track B — HTTP/runtime docs

Files: `runtime.md`, `http2.md`, `http3.md`, `response-planning.md`,
`primitives-api.md`.

- `runtime.md`: add compact 243/244/245/253 notes + 254/257–258 async
  pointers; leave 250–252/255–256 unmentioned unless directly relevant.
- `http2.md` / `http3.md`: add the Plan 253 ledger pointer (core H2
  execution retained; H3 Alt-Svc post-pass / shared-kernel split).
- `response-planning.md`: fix authority wording (implementation lives in
  `eggserve-static`, core is a facade; Plan 245 ownership note); fix the
  `FileRange` literal to private fields constructed via `try_new`/`new`
  and read via accessors (verify against
  `crates/eggserve-primitives/src/` first).
- `primitives-api.md`: fix the `secure_root.rs` snippet connotation (type
  lives in `eggserve-static`, not core `crate::fs`); add a short
  246/251–256 interop-fidelity pointer for the existing `interop.rs` row.

## Track C — Security/ops/frontend docs

Files: `security-model.md`, `error-taxonomy.md`, `structured-logging.md`,
`tls.md`, `configuration.md`, `eggserve-python.md`,
`testing-and-conformance.md`.

- `tls.md`: fix the `bin/src/tls.rs` re-export target to
  `pub use eggnet_tls::*` (verify against `crates/eggserve-bin/src/tls.rs`;
  matches `eggserve-bin.md` + `overview.md`); reword `Location:` lines so
  neutral ownership is `eggnet-tls` with core as transport glue; add
  194/195 + 213/220 cross-refs.
- `error-taxonomy.md`: fix ownership locations (canonical taxonomy now in
  `eggserve-server` / `eggserve-primitives`, core facades); reconcile the
  row counts (`PathRejection` 17 with `ControlCharacter`;
  `RequestBodyError` 14 with `InvalidTrailers`/`TrailersNotReady`).
- `security-model.md`: fix the trust-boundaries diagram label
  (`eggserve-core (policy layer)` → primitives policy / server runtime /
  static authority with core as compatibility umbrella).
- `structured-logging.md`: fix the lede (authority is
  `eggserve-server::ops`, core keeps a facade); fix the `0.1.0` example
  version to `0.2.0`.
- `configuration.md`: add the Plan 243 pointer (durable shutdown state,
  drain semantics).
- `eggserve-python.md`: add 246/252 (stub fidelity) + 254/257–258 (async
  lifecycle/streaming/suppressed-body permit) pointers; fix the structure
  diagram if it still shows a flat `server.rs` instead of `server/`
  submodules.
- `testing-and-conformance.md`: add the 243–258 suite inventory
  (`direct_h1_parity` / auto-H1 delegation (249), static-authority (245),
  overlap-guard (253), typing/async fixtures (246/252/254/257–258)) or a
  freshness note if counts are stated.

## Verification

- All edited-file links resolve; `overview.md` index untouched unless a
  blurb becomes false.
- `python3 scripts/check-crate-topology.py` passes.
- `python3 scripts/verify-conformance-matrix.py` passes.
- `git status` shows only intended `architecture/*.md` + this plan.
