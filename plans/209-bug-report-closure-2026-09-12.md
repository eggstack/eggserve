# Plan 209 — Bug Report Closure (2026-09-12 Interrogation)

## Status

**COMPLETE — 2026-09-12.**

## Scope

Resolve all actionable findings B-01…B-07 in the temporary `bugs.md`
interrogation dated 2026-09-12 (commit `7751ad8`; all suites green per the
report). Keep changes minimal, preserve existing contracts, no feature
additions or optimizations. Delete `bugs.md`, verify locally, commit and push
on `main`.

## Fixes

1. **B-01 — Dead `PolicyMode` enum** (`eggserve-core/src/policy.rs:6-11`):
   deleted the unused `pub(crate) enum PolicyMode { Strict, Compat }` plus its
   `#[allow(dead_code)]`. No constructor, match site, or re-export existed.
   Also removed the stale `PolicyMode | internal | pub(crate)` row in
   `docs/api-stability.md`. No behavior change.

2. **B-02 — `expect()` in H3 CLI path** (`eggserve-bin/src/lib.rs:333-339`):
   replaced both `expect()` calls on `args.tls_cert`/`args.tls_key` with a
   graceful `let (Some(cert), Some(key)) = … else` that emits the same
   `ProcessStarting` error event used by the guard 30 lines above
   (`--http3 requires --tls-cert (and optionally --tls-key)`) and returns `1`.
   Currently unreachable (the earlier `tls_config.is_none()` guard plus the
   `args.rs` combined-PEM normalization guarantee `Some`/`Some`), but no
   longer ordering-coupled. No behavior change on reachable paths.

3. **B-03 — `ServerHandle::local_addr()` panics on Unix-only servers**
   (non-breaking, docs-only): kept the signature (changing to
   `Option`/`Result` would be a breaking API change and would turn every
   `local_addr()` call site into a warning/error under `-D warnings` if
   deprecated). Instead documented the contract where callers look:
   added an explicit `# Panics` section on `local_addr()` pointing generic
   callers to `tcp_local_addr()`/`endpoints()`, clarified the
   `ServerHandle` bullet list, and noted the Unix-only panic in
   `docs/api-stability.md` and `docs/release-contract.md`
   (`architecture/runtime.md` already recorded it). No behavior change.

4. **B-06 — Lock-poisoning containment** (docs-only,
   `server/connection/activity.rs`): documented the existing intentional
   policy on `ConnectionActivity`: `state`/`response_poll_progress` mutexes
   are locked without panicking; on poisoning the update is skipped
   (progress treated as no-progress, stall checks report "not stalled") and
   the outer connection/write timeouts remain the backstop. The I/O path
   never panics on a poisoned lock. No behavior change.

Intentionally not changed (verified, no in-tree action per the report):

- **B-04 (H3 upstream data-loss blockers)** — `hyperium/h3#338` open plus the
  `#262` remainder; stack stays pinned at `h3 0.0.8` / `h3-quinn 0.0.10` /
  `quinn 0.11.11`. Requires an upstream release + new scoped promotion plan;
  no hand-rolled QUIC framing workaround.
- **B-05 (H3 generic `:protocol`)** — no API in pinned `h3 0.0.8`; waits on
  an upstream dependency upgrade, no in-tree shim.
- **B-07 (Windows trusted/local-content scope)** — OS behavior (NTFS rejects
  the two open-descendant root-rename cases); docs language already scopes
  Windows correctly. Qualification via manual `platform-qualification.yml`
  only.
- **O-01…O-08 (optimizations)** — pure perf notes with no correctness impact
  (per-chunk alloc, ETag/`Date`/`Alt-Svc`/header formatting, per-request
  `Arc` fan-out, progress-vec scan, denylist scan). Deferred per the
  `benchmarks/README.md` claims policy: only with profiling evidence, and
  the `Date` cache (O-03) last as riskiest. Explicitly not claimed: release
  `opt-level`/LTO (Cargo defaults already apply) and Tower per-request
  clone/`Box::pin` (by-design isolation, Plan 200).
- **F-01 (dismissed false positive)** — `FileRange` fields are private;
  struct-literal construction outside the module is impossible. No action.

No test changes were needed: all fixes are dead-code removal or
docs/graceful-unreachable-path changes with identical observable behavior on
reachable paths.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`
- `cargo test --workspace`
- `python3 scripts/verify-conformance-matrix.py`
- `python3 scripts/check-python-release-metadata.py`
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`
- `cargo test --doc -p eggserve-core`
- `./scripts/verify.sh fast` (routine chain incl. `http2,tls`, `http3,tls`,
  `tls` bins + python crate check)

## Completion criteria

- [x] B-01, B-02 fixed; B-03, B-06 documented; B-04/B-05/B-07 verified as
  no in-tree action; O-01…O-08 explicitly deferred; F-01 needs no action;
- [x] verification passes;
- [x] `bugs.md` deleted;
- [x] committed and pushed from `main`.
