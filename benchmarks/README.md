# Benchmark methodology and evidence

This directory holds benchmark baselines and qualification snapshots. Numbers
here are evidence, not marketing: absolute values are platform- and
machine-specific and must never be copied into README or release prose as
headline claims. Every release/documentation performance claim must name the
qualified profile and point at the evidence files below.

## Evidence index

| Location | Content |
|----------|---------|
| `088-baseline/results.json` | Plan 088 handler-latency baseline (macOS arm64, warm cache): GET/HEAD/range/conditional sizes, chunk throughput, directory-listing scaling. The Criterion harness that produced it is historical. |
| `088-baseline/*-audit.md` | Plan 088 allocation/accept-loop/body/range/listing/TLS/comparative audits. Historical snapshots; stale file paths inside them refer to the tree at that time. |
| `binary-size.md` | Plan 109 distribution artifact sizes (release vs `dist` profiles, wheel members) plus a current-thread suitability smoke. Profile-aware: never compare `release` against `dist` as a code-size delta. |
| `168-qualification/results.json` | Plan 168 loopback throughput smoke (Linux x86_64, release CLI): 1 KiB and 1 MiB static GETs, 16 keep-alive workers, 3 trials, 0 errors, server RSS. Reproduce with `benchmarks/168-qualification/loopback_smoke.py`. |

## Method

Representative workloads (per Plan 168) cover the qualification matrix where
applicable: built-in static service, buffered/known-length/unknown-length
custom services, TCP `Server`, caller-owned connection driver, Python native
fast path, Python low-level callback/streaming service, and TLS variants of
representative native/Python paths. Response sizes span empty through 16 MiB
where infrastructure supports it; request bodies span none through
over-limit/disconnected; connection patterns span one-shot, keep-alive
sequential, high concurrency, idle hoards, admission saturation, request-count
bounds, slow header/body/reader, and shutdown under load.

For each workload record: source commit SHA and lockfile, Rust/Python
versions and build profile, OS/CPU/arch, client tool and command,
concurrency/reuse/duration/request count, throughput (req/s and bytes/s),
median/p95/p99 latency, CPU, steady-state and peak RSS, allocations where
supported, task/thread counts, fd/handle counts, errors/timeouts/rejections,
and EggServe admission/timeout counters. Run repeated trials, report
variance, and preserve raw files here — never promote one best number.

## Regression policy

- Correctness and resource-limit regressions are hard failures regardless of
  throughput.
- Statistically meaningful throughput/latency regressions above the accepted
  Plan 088 baseline-relative threshold require explanation or rollback.
- Intentional security limits may cost a microbenchmark when the tradeoff is
  measured and documented.
- Noisy absolute-timing gates stay out of PR CI; they run as
  manual/nightly/release qualification. Small deterministic smoke benchmarks
  (exact bytes, exact status, exact header presence) may gate CI. The
  168-qualification snapshot demonstrates why: same machine, same binary,
  two runs an hour apart differed ~1.7x in absolute RPS (frequency/background
  state) with zero errors either way.

## Comparative baselines

- **Python `http.server`**: the migration baseline is current CPython
  `python -m http.server` for simple static GET/HEAD and concurrency
  behavior, matched on bind/file/protocol/concurrency/page-cache/TLS scope.
  It answers "what changes when substituting EggServe", not a victory chart.
- **Granian/Gunicorn and other app servers**: architectural references only,
  never direct performance baselines unless the full application/protocol/
  worker setup is controlled and documented. No cross-product headline
  numbers without a dedicated methodology review.

## Claims policy (Plan 168)

Allowed only with named profile + evidence: hardened static/server runtime
for reverse-proxy deployments; qualified limited direct-TLS profile;
reusable canonical HTTP/1 service/connection substrate; bounded
caller-owned-stream profile behind a separate WAF/rate-limiting layer;
Python `http.server`-shaped compatibility facade with a Rust HTTP runtime;
Python low-level synchronous service substrate.

Do not claim: nginx/Caddy replacement; bare-Internet DDoS resistance;
anonymity or un-fingerprintability; ASGI/WSGI/Gunicorn/Granian parity;
HTTP/2/3; performance superiority without a controlled published comparison.
