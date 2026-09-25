# Plan 294 — Direct Tower and footprint baseline

Evidence-only milestone: no production changes. Fixtures, raw captures, and
the decision record gating Milestones 295/296.

## Fixtures

`crates/eggserve-server/tests/direct_tower_baseline_294.rs` (tower-gated):

- Non-ignored CI controls: native vs Tower vs Axum 1 KiB parity, header-count
  sensitivity parity (1/16/64), SSE-like 32-chunk streaming parity, POST echo
  parity, terminal-trailer convergence, Stream-policy connection-behavior
  record, streaming cancellation recovery.
- Ignored timing matrix `baseline_timing_matrix` (explicit release runs
  only): interleaved-round sequential keep-alive GETs, header sweep, SSE-like
  streams and 1 MiB responses over fresh connections.

## How to reproduce

```bash
# Parity controls (routine CI shape)
cargo test -p eggserve-server --no-default-features --features tower \
  --test direct_tower_baseline_294
cargo clippy -p eggserve-server --no-default-features --features tower \
  --lib --tests -- -D warnings

# Timing matrix (release only; stdout JSON lines start with BASELINE294)
PROFILE_294=release cargo test --release -p eggserve-server \
  --no-default-features --features tower \
  --test direct_tower_baseline_294 baseline_timing_matrix \
  -- --ignored --nocapture

# Dependency graphs
cargo tree -p eggserve-server --no-default-features -e no-dev --prefix none | sort -u
cargo tree -p eggserve-server --no-default-features --features tower -e no-dev --prefix none | sort -u
cargo tree -e features -p eggserve-server --no-default-features --features tower
```

Stripped size fixtures were throwaway manifests (one minimal `Service` bin,
one minimal Tower-service bin, one Axum-router bin; identical 1 KiB route)
built with a `dist`-equivalent profile (opt-z, fat LTO, 1 codegen unit,
stripped). Recipe: temporary crate with
`eggserve-server = { path = "<repo>/crates/eggserve-server",
default-features = false [, features = ["tower"]] }`, `cargo build --profile
dist`, verify `file` reports stripped, execute once. Manifests are not
retained; the profile definition and exact byte counts are in
`raw/sizes-dist.txt`.

## Headline results (release, i9-9900K loopback, sequential)

| Case | Native | Tower | Axum |
|---|---|---|---|
| GET 1 KiB keep-alive | ~13.7k rps, p50 0.069ms | ~11.8k rps, p50 0.083ms | ~11.3k rps, p50 0.086ms |
| SSE-like 32 chunks | ~8.8k streams/s | ~8.3k streams/s | — |
| 1 MiB response | ~2.86k rps | ~2.9–3.0k rps (noise) | ~2.8k rps |

Stripped dist: native 936608 B, tower 951480 B (+1.6%), axum 1037768 B.
No-dev package delta for `tower`: exactly `tower-layer` + `tower-service`.

## Structural findings (not noise)

- **F1**: every Tower response is chunked, even fully-buffered bodies with
  exact size hints (`response_from_http_body` streaming path). Content-Length
  never emitted on the Tower path.
- **F2**: `Stream` policy + service never polls body → `connection: close`
  even on bodyless GET. Default `TowerToEggserve::new()` + Axum GET cannot
  reuse keep-alive connections.
- **F3** (correctness, out of campaign scope): H1 response trailers never
  reach the wire on any path — Hyper requires a `Trailer` head declaration
  that EggServe strips and never synthesizes. Native/Tower converge on the
  drop; fixing needs a separate scoped plan.

## Decision record

| Candidate | Measured/mechanical cost | Decision for 295/296 |
|---|---|---|
| Known-length Tower response fast path (F1) | Chunk framing on every Tower response; exact `size_hint` available for buffered bodies | PROCEED in 295 (P1) |
| Unconsumed-empty-body keep-alive close (F2) | Keep-alive reuse prohibited for the default Tower shape | PROCEED in 295 (P2) with framing-safety stop conditions; DEFER to corrective plan if lifecycle redesign required |
| Validate/project header split, unsafe aliasing, Service redesign | ~13µs/request total budget shared with necessary work | NO-GO / DEFER |
| `tower-layer` removal | Mechanically unused by production code (only tests + doc mention); 1 package node | PROCEED in 296 |
| Tokio `fs` / file-body gating | Unconditional today; link impact unproven (LTO may already erase) | PROCEED in 296 as experiment, KEEP only on measured benefit |
| Tunnel/upgrade gating | Always compiled; hot-path/link cost unattributed | PROCEED in 296 as experiment, NO-GO default |
| H1 response-trailer restoration (F3) | Silent trailer-section loss | NO-GO for 295/296; separate correctness plan required |

Full tables: `results.json`. Raw: `raw/`.
