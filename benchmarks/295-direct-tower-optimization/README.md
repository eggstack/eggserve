# Plan 295 — Direct Tower hot-path optimization (P1 + P2)

Bounded to the two 294 PROCEED candidates. No other hot-path rework.

## P1 — Known-length Tower response fast path

`crates/eggserve-server/src/interop.rs` (`response_from_http_body`): when the
ecosystem body reports an exact size hint, the adapted canonical stream is
declared with `with_known_length_and_trailers` so the runtime emits
`Content-Length`. The transport verifies the exact byte count and fails
closed (truncated close, no second response) on over/under-run — declare,
never trust. Bodies without an exact hint keep the unknown-length chunked
path; nothing is buffered.

Effect: Tower/Axum buffered responses now carry `Content-Length`
(`content-length: 1024` where the baseline sent chunked); streaming bodies
unchanged.

## P2 — Provably-empty request bodies complete as empty

`crates/eggserve-server/src/connection/pipeline.rs` (Buffer/Stream branch):
when Hyper already reports `is_end_stream()` before any body poll, the body
completes as `RequestBody::empty()` instead of wrapping the transport
stream. An unconsumed drop then stays `Complete` rather than abandoning a
network body, preserving keep-alive reuse for bodyless requests under every
policy (matching Reject-path behavior). Any case with potentially unread
wire bytes (chunked framing, declared length, trailers) keeps the wrapped
path, where an unconsumed drop still forces close.

Effect: the default `TowerToEggserve::new()` + Axum GET shape reuses
keep-alive connections (previously every response closed). Native Stream
services benefit identically — the rule was shared lifecycle behavior, not
Tower-specific.

## A/B (release, interleaved 294 matrix; raw/ retained)

| Case | Baseline | Candidate |
|---|---|---|
| Tower 1 KiB (3 rounds) | ~11.8k rps, p50 0.083ms | ~12.7k rps, p50 0.074ms |
| Axum 1 KiB | ~11.3k rps, p50 0.086ms | ~13.0k rps |
| Native 1 KiB control | ~13.7k rps, p50 0.069–0.070ms | unchanged |

Both candidates KEEP. Full `verify.sh fast` (24/24) + conformance matrix +
core tower lanes green with the changes landed.

## Explicitly not done

- Validate/project header split, unsafe aliasing, Service redesign: NO-GO.
- `Arc<Mutex>` trailer rendezvous / dedicated no-trailer machine: DEFER
  (SSE overhead ~5%; risk without measured budget).

Reproduce: `cargo test -p eggserve-server --no-default-features --features
tower --test direct_tower_hotpath_295` plus the 294 matrix command in
`benchmarks/294-direct-tower-baseline/README.md` with `PROFILE_294=release-candidate`.
