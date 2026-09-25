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
| `170-closure/results.json` | Final Plan 170 same-machine evidence: native static scaling, native buffered/streaming services, installed-wheel low-level Python, admission recovery, TLS, and CPython substitution. Reproduce with `benchmarks/170-closure/benchmark.py`; caller-owned duplex output is recorded separately by the ignored Rust benchmark test. |
| `227-current-head/` | Plan 227 current-HEAD baseline: native Rust keep-alive client, in-process body/frame matrix, syscall-profile fallback when hardware CPU profiling is unavailable, and machine/lock/build capture. |
| `231-optimization-closure/` | Plan 231 same-machine candidate closure: post-optimization A/B results, deterministic qualification commands, and keep/revert decisions for Plans 228–230. |
| `232-corrective/` | Plan 232 corrective proof: explicit bounded file reads, forced-over-capacity regression, 64/128 KiB live static comparison, exact range probes, and representative TLS evidence. |
| `233-evidence-polish/` | Plan 233 provenance polish: retained per-trial native/range/TLS JSON behind the 128 KiB decision, closure-SHA CI record, and the mechanically derived aggregate. Future manual performance qualification must retain compact per-trial JSON here rather than only aggregate reductions of discarded captures. |
| `234-fixed-cost-baseline/` | Plan 234 current-head fixed-cost baseline: native keep-alive measurements, syscall fallback profiles, environment/lock provenance, and evidence-gated decisions for Plans 235–239. |
| `240-fixed-cost-closure/` | Plan 240 same-machine candidate closure: retained native A/B capture, per-track keep/revert/defer decisions, and local CI/qualification provenance. |
| `241-fixed-cost-evidence-corrective/` | Plan 241 corrective closure: per-trial custom H1/path/TLS/installed-wheel Python/slow-stream evidence, before/after Unix resolver traces, truthful unavailable/deferred classifications, and exact-SHA CI provenance. |
| `294-direct-tower-baseline/` | Plan 294 direct Tower/Axum baseline: identical native/Tower/Axum fixtures, interleaved-round release timing (buffered + SSE-like + 1 MiB), no-dev graphs, stripped dist sizes, and the PROCEED/NO-GO decision record for Plans 295/296 (findings F1 always-chunked Tower responses, F2 Stream-policy keep-alive close, F3 H1 response-trailer wire gap). |
| `295-direct-tower-optimization/` | Plan 295 candidate A/B: P1 known-length fast path + P2 provably-empty completion (tower p50 0.083→0.074ms), per-candidate KEEP/NO-GO/DEFER dispositions, retained release-candidate trials. |
| `296-direct-profile-footprint/` | Plan 296 capability split: tower-layer deactivation evidence (−1 graph node, link-neutral), file-body/tunnel NO-GO symbol attribution (~35 KB/~30 KB upper bounds vs API churn), package-consumer numbers, semver classification. |
| `297-application-server-qualification/` | Plan 297 campaign closure: EggPool-geometry fixture, extended matrix (c16, SSE-128, POST/JSON, slow streams, RSS), footprint/freeze records, KEEP/DEFER release decision (no publication; next server release minor). |

## Method

Representative workloads (per Plans 168 and 170) cover the qualification
matrix where applicable: built-in static service, buffered/known-length/unknown-length
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

Plan 170 is the final performance-evidence extension. Its required Linux
x86_64 session uses explicit runtime limits, one excluded warm-up, three
measured trials, 1/16/64/256 static keep-alive concurrency, 16/64 custom
service concurrency, installed-wheel Python callbacks, separate TLS handshake
churn, and a same-session CPython substitution baseline. The caller-owned
driver is measured in-process over `tokio::io::duplex`; it is not network RPS.
Unavailable arm64 hardware is recorded as performance-unqualified rather than
silently represented by x86_64 numbers.

Plans 227–240 extend that evidence without changing the claims policy. Plan
227 adds a dependency-free Rust client so small-response conclusions are not
limited by CPython client overhead, plus an in-process body/frame matrix and a
best-effort syscall profile. Plans 228–230 use the captured costs to simplify
the direct H1 state path, file-stream reads, metadata validation, and disabled
observability paths. Plan 229's selected default is a 128 KiB file-stream
chunk; the configured `max_file_streams * stream_chunk_size` bound remains the
memory authority. Plan 231 records the candidate comparison and reruns the
full correctness/resource matrix. Plan 232 rechecks the 128 KiB choice with
live 64/128 KiB throughput/resource evidence, exact range probes, and
representative TLS captures. Plan 233 retains the per-trial native, range,
and TLS captures behind that decision plus the closure-SHA CI record, without
changing any default. These are manual qualification artifacts, not
timing gates.

Plans 234–240 continue the same discipline for fixed costs. The retained
changes use canonical representations and lazy materialization where public
behavior is unchanged: request-target slices, lazy wire-trailer state,
borrowed Unix root-FD traversal, normalized-path and range-parser fast paths,
the generic H1 service adapter, zero-tunnel activity checks, and lazy Python
target/header views. Request-scoped interim state remains separate (Plan 238
is closed as NO-GO), and the dedicated Python stream producer remains a
bounded isolation tradeoff (Plan 239 DEFER). The absence of a supported
allocator counter on the baseline host is recorded rather than replaced with
invented allocation counts.

Plan 241 is the evidence-only corrective for the narrower-than-written Plan
240 matrix. Its compact records add custom H1, static response-shape,
established-TLS, installed-wheel Python callback, slow synchronous stream,
and focused Unix resolver syscall evidence. TLS and metadata-heavy results
are classified as neutral where variance dominates; unavailable peer-chain
exposure is not inferred. The Plan 238 NO-GO, Plan 239 producer DEFER, and
metadata-sharing DEFER remain unchanged.

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

## Claims policy (Plans 168 and 170)

Allowed only with named profile + evidence: hardened static/server runtime
for reverse-proxy deployments; qualified limited direct-TLS profile;
reusable canonical HTTP/1 service/connection substrate; bounded
caller-owned-stream profile behind a separate WAF/rate-limiting layer;
Python `http.server`-shaped compatibility facade with a Rust HTTP runtime;
Python low-level synchronous service substrate.

Do not claim: nginx/Caddy replacement; bare-Internet DDoS resistance;
anonymity or un-fingerprintability; ASGI/WSGI/Gunicorn/Granian parity;
HTTP/2/3; universal performance superiority; or ratios from the CPython
substitution baseline as production-server marketing. Performance claims must
name the profile, workload, machine, and evidence file; absolute RPS/latency
never gates PR or routine CI.
