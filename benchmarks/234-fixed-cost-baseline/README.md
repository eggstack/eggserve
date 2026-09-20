# Plan 234 — Fixed-cost baseline and profiling

This is the post-Plan-233 baseline captured before the Plan 235–239 source
changes. It is a same-machine engineering capture, not a timing gate or a
universal performance claim.

## Reproduction

```sh
cargo build --release --locked -p eggserve-bin
cargo build --release --locked --example streaming_service -p eggserve-core
python3 benchmarks/227-current-head/native_harness.py --trials 3 \
  --output benchmarks/234-fixed-cost-baseline/raw/native-static-results.json
python3 benchmarks/227-current-head/profile_harness.py \
  --output-dir benchmarks/234-fixed-cost-baseline/profiles
```

The native capture uses the dependency-free Rust keep-alive client from Plan
227. The profile fallback uses `strace -f -c`; hardware counters are blocked on
this host (`perf_event_paranoid=4`) and no heap profiler is installed. The
raw directory retains all three trials for each of nine static cases. The
profile directory retains 1 KiB, 1 MiB, and application-stream syscall
summaries.

## Baseline observations

- 1 KiB static medians were 5,190 / 21,185 / 19,786 RPS at concurrency
  1 / 16 / 64, with zero errors across three trials.
- The 1 KiB profile includes one `fcntl`, `openat`, `newfstatat`, and `statx`
  group per request, plus the security-required metadata and close work. The
  non-root resolver also showed the root descriptor `fcntl` duplication that
  Plan 236 targets.
- Source inspection mechanically identified two owned target strings (path
  and query), an active-to-complete transition for already-terminal bodies,
  an eager wire-trailer slot for empty/fixed bodies, and a temporary vector in
  `HeaderBlock::get_unique`.
- Python request construction eagerly copied raw/path/query bytes and both
  ordered/text header views. The synchronous stream bridge used one dedicated
  native producer thread per active stream and a bounded 16-item channel.
- No supported allocator counter was available. Allocation conclusions in
  the candidate records are therefore mechanical or explicitly marked
  unavailable; no numeric allocation count is inferred from timing.

See `results.json` for provenance and `decisions.md` for the handoff to the
implementation plans.

Historical note: the custom-service, response-shape, established-TLS,
installed-wheel Python, slow-stream resource, and focused before/after syscall
measurements required by the later acceptance matrix were not present when
this baseline was executed. Plan 241 records them as completed corrective
evidence; this baseline record is not being rewritten to claim otherwise.
