# Plan 297 — Direct application-server qualification and closure

Campaign capstone: qualifies the retained 295 (P1, P2) + 296 (WP-A) changes
against native H1, Tower/Axum, and an EggPool-shaped streaming consumer.
No production changes in this milestone (reverts only if qualification
failed — none did).

## Downstream-like consumer (WP-E)

`crates/eggserve-server/tests/application_server_qualification_297.rs`:
caller-bound `TcpListener` → direct `Server` + `TowerToEggserve`
(default constructor) → Axum `Router` with `/health` (small JSON),
`/echo` (bounded 64 KiB body), `/events` (gated SSE stream) → controlled
shutdown via `into_parts`. No EggPool logic imported.

## Qualification evidence

- Correctness/security: `verify.sh full` 31/31 lanes green (one
  environmental incident: disk-full linker bus error; remediated by freeing
  regenerable artifacts, then re-ran green — see closure §4).
- Performance/resource: 294 interleaved matrix + 297 extended matrix
  (c16, SSE-128, POST/JSON, 10 slow streams, RSS markers) — full tables in
  `results.json`, raw trials in `raw/`.
- Footprint: `raw/footprint.txt` (dist sizes, counts, combinations).
- Freeze: `raw/freeze.txt` (baseline/candidate SHAs, toolchain, limits).

## Release decision

KEEP P1, P2, WP-A. NO-GO dispositions stand. No publication in this
campaign; next `eggserve-server` release is MINOR (0.4.0) when cut,
driven solely by the WP-A feature edge. Registry-consumer shape proven via
local-registry package dry-run (no registry publication to prove).

Reproduce: `cargo test -p eggserve-server --no-default-features --features
tower --test application_server_qualification_297` and
`PROFILE_297=release-qual cargo test --release ... extended_timing_matrix
-- --ignored --nocapture`, plus `./scripts/verify.sh full`.
