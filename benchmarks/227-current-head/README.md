# Plan 227 — Current-HEAD baseline and profiling

This directory records the pre-optimization baseline captured from commit
`4fda8a3872684a0f5d1a6ca80de30c452f135080` on Linux x86_64. Plan 170 remains
historical; its compatible workload families were rerun here without changing
`benchmarks/170-closure/`.

Reproduction:

```sh
cargo build --release --locked -p eggserve-bin
cargo build --release --locked --example streaming_service -p eggserve-core
python3 benchmarks/170-closure/benchmark.py --trials 3 --skip-tls \
  --output benchmarks/227-current-head/raw/plan170-compatible-results.json
python3 benchmarks/227-current-head/native_harness.py --trials 3 \
  --output benchmarks/227-current-head/raw/native-static-results.json
rtk proxy cargo test --release --locked -p eggserve-core \
  --test plan227_body_benchmark -- --ignored --nocapture
python3 benchmarks/227-current-head/profile_harness.py
```

The native client is dependency-free Rust standard-library TCP code. Python
only starts the server and records JSON, so small-response conclusions do not
depend on CPython's `http.client` loop. The runtime limits match the Plan 170
native static profile: 512 connections, 512 file streams, 512 in-flight
requests, 64 KiB parser buffer, 100 headers, 32 KiB aggregate headers, and
8 KiB request-target ceiling.

## Findings and handoff

- The CPython client was a ceiling for small responses: the compatible Plan
  170 run measured approximately 6.0k/7.6k/7.5k RPS at concurrency
  1/16/64 for 1 KiB static responses, while the native client measured
  approximately 5.2k/21.4k/18.8k RPS under the same server profile. The
  native path is therefore required for Plans 228–231 small-response claims.
- Direct H1's `response_poll_progress` vector is not read by its timeout
  driver. `ConnectionActivity::last_write`, updated by `ProgressIo::record_write`,
  is the only H1 write-stall authority. Compatibility core retains its
  separate vector because its H2 producer-stall path consumes it.
- In-process file adapter measurements showed frame/read overhead falling as
  chunks grew. On this host, 128 KiB was the best common point for 1 MiB and
  16 MiB files; 256 KiB was not consistently better for 1 MiB. Plan 229 uses
  128 KiB as the default, with the bounded default payload at
  `32 * 128 KiB = 4 MiB` before allocator/runtime overhead.
- Zero-filling was part of the old `vec![0; chunk_len]` path. The replacement
  uses safe `BytesMut` capacity plus `read_buf`, freezes only initialized bytes,
  and preserves short-read/EOF behavior. No unsafe code or buffer pool was
  added.
- The native small-response path is materially above the CPython client at
  concurrency 16/64, so the old apparent equivalence was client-side
  saturation rather than proof of server-path equivalence.
- Per-frame producer progress was mechanically dead for direct H1; the
  per-request state wrapper also cloned many independent handles. Plan 228
  removes the dead H1 state and captures immutable connection state behind one
  `Arc<PipelineState<S>>`.
- Runtime conversion now skips temporary origin `Date` creation when an
  observability context marks a contextual conversion; runtime finalization is
  the sole policy authority. Standalone conversion retains its documented
  default Date behavior.
- Disabled/filtered event construction was not allocator-profiled because
  `perf` is blocked by this host's `perf_event_paranoid=4`. The no-op and
  filtered sink capability is mechanically tested, and stream lifecycle debug
  events now use the lazy path.

## Profiler limitation

`perf` is installed but cannot access performance counters on this host. The
profile harness therefore records `strace -f -c` syscall summaries for 1 KiB
static, 1 MiB static, and 1 MiB application-stream paths under `profiles/`.
These summaries are supporting evidence, not CPU-symbol profiles. The raw
results explicitly record the unavailable `perf` capability and no allocation
counts are invented.

See `results.json` for the machine-readable capture index and the `raw/`
directory for compact trial output.
