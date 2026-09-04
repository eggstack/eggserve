# Plan 168 — Production, Embedding, Privacy, and Performance Qualification

## Status

**CLOSED — CORRECTNESS/RESOURCE/PRIVACY QUALIFICATION AND PLAN 170 PERFORMANCE EVIDENCE.**

Plan 167 is optional and does not block core qualification when closed as a no-go.

## Closure record

Correctness, resource, privacy, and the recorded qualification snapshot were
closed in commit `b2462a6df2e571c1fc85bf637601a761469b38e2`; the evidence-SHA
correction was recorded in commit `4922227f55502885621620aaa4c915458055ec84`.
The broader performance matrix and final claims closure are intentionally
deferred to Plan 170 rather than implied by the narrower existing snapshot.

Plan 170 subsequently completed the deliberately smaller representative
performance matrix and final claims closure; it did not retroactively expand
the original Plan 168 snapshot.

## Final closure record

Plan 168's deterministic correctness, resource, privacy, and failure-recovery
qualification remains evidenced by the suites and the original
`benchmarks/168-qualification/results.json` snapshot. That snapshot is a
narrow Linux x86_64 release-CLI static GET smoke and is preserved unchanged.
The expanded Plan 170 evidence at `benchmarks/170-closure/results.json` adds
native static concurrency scaling, buffered and known/unknown streaming,
installed-wheel low-level Python, caller-owned duplex, representative TLS,
CPython substitution, and admission/recovery measurements. The two artifacts
are complementary; neither creates an absolute-timing CI gate or supports
universal performance/edge-server claims.

## Goal

Produce reproducible evidence that the expanded EggServe runtime remains HTTP-correct, bounded under hostile/slow clients, useful at high concurrency, transport-neutral for embedding, and predictable under the anonymity-sensitive response policy.

This plan is a qualification/closure phase, not a feature phase. It builds on the benchmark, soak, fuzz, conformance, and production-profile work from Plans 070, 072, 088, 089, 090, 109, 119, and later closure plans.

## Claims policy

Every release/documentation claim must name the qualified profile and evidence.

Allowed claim shapes after evidence exists:

- hardened static/server runtime for reverse-proxy deployments;
- qualified limited direct-TLS profile;
- reusable canonical HTTP/1 service/connection substrate;
- bounded caller-owned-stream profile suitable for embedding behind a separate WAF/rate-limiting layer;
- Python `http.server`-shaped compatibility facade with a Rust HTTP runtime;
- Python low-level synchronous service substrate.

Do **not** claim:

- nginx/Caddy replacement;
- bare-Internet DDoS resistance;
- anonymity or un-fingerprintability;
- ASGI/WSGI/Gunicorn/Granian feature parity;
- HTTP/2/3 support;
- performance superiority without a controlled published comparison.

## Qualification matrix

Run the new evidence across these server paths where applicable:

1. Rust built-in static service;
2. Rust custom `Service` with buffered response;
3. Rust custom `Service` with known-length stream;
4. Rust custom `Service` with unknown-length stream;
5. TCP `Server` convenience runtime;
6. caller-owned non-TCP connection driver;
7. Python stdlib facade native static fast path;
8. Python low-level synchronous callback service;
9. Python low-level streaming response service;
10. TLS variants of representative native/Python paths.

If Plan 167 ships an adapter, qualify it separately; do not mix adapter failures into core HTTP claims.

## Profile matrix

### Reverse-proxy production

Use documented production defaults and simulate proxy-like persistent HTTP/1 connections. The benchmark client is the proxy analogue; no real nginx/Caddy dependency is required for every run.

### Direct TLS

Exercise current rustls/native TLS profile with handshake churn and established keep-alive workloads.

### Embedded anonymity-sensitive origin

Drive the Plan 163 canonical connection API over a deterministic non-socket test transport with shared Plan 164 admission and Plan 165 privacy policy. Treat a separate WAF/rate limiter as an architectural precondition, not something simulated inside EggServe.

## Workload classes

Reuse Plan 088 static-file sizes and add application-service cases.

### Response sizes

At minimum:

- empty;
- 1 KiB;
- 16 KiB;
- 128 KiB;
- 1 MiB;
- 16 MiB;
- larger file/stream where test infrastructure supports it.

### Request bodies

At minimum:

- no body;
- small buffered body;
- 1 MiB-class body within configured limit;
- chunked streaming body;
- body at limit;
- body one byte above limit;
- incomplete/disconnected body.

### Connection patterns

- one request per connection;
- keep-alive sequential requests;
- high connection concurrency;
- many idle keep-alive connections with a smaller active-request set;
- service-admission saturation below connection saturation;
- max-requests-per-connection close;
- slow header sender;
- slow request-body sender;
- slow/stalled response reader;
- graceful shutdown under active load.

## Performance metrics

For each representative workload record:

- source commit SHA and dependency lock;
- Rust/Python versions and build profile;
- OS/CPU/architecture;
- client tool/version and command;
- concurrency, connection reuse, duration, and request count;
- throughput (requests/s and bytes/s where meaningful);
- median/p95/p99 latency;
- CPU utilization/time;
- steady-state and peak RSS/working set;
- allocation count/bytes where tooling supports it;
- task/thread count;
- open fd/handle count;
- errors/timeouts/rejections;
- relevant EggServe admission/timeout counters.

Run repeated trials and report variance. Preserve raw result files or machine-readable summaries under the existing benchmark/evidence convention rather than copying one best number into README prose.

## Comparative baselines

### Python `http.server`

Use current CPython `python -m http.server` as a migration baseline for simple static GET/HEAD and connection-concurrency behavior. Match bind, file, protocol version, client concurrency, page-cache state, and TLS/plaintext scope where possible.

The comparison should answer “what changes when a user substitutes EggServe for stdlib serving,” not serve as a synthetic victory benchmark.

### Higher-level Python servers

Granian/Gunicorn may be referenced architecturally but are not direct performance baselines unless the exact application/protocol/worker setup is controlled and documented. They include application-server/process/protocol responsibilities outside EggServe's scope.

Do not put cross-product headline numbers in release docs without a dedicated methodology review.

## Regression policy

Reuse/extend Plan 088's baseline-relative thresholds rather than inventing universal RPS targets.

Required policy:

- correctness/resource-limit regressions are hard failures regardless of throughput;
- statistically meaningful throughput/latency regressions above the existing accepted threshold require explanation or rollback;
- intentional security limits may reduce a microbenchmark and are acceptable when the tradeoff is measured/documented;
- performance tests that are too noisy for PR CI run as manual/nightly/release qualification, while small deterministic smoke benchmarks may gate CI.

Avoid flaky CI gates based on public-cloud absolute timing.

# Track A — Streaming response correctness under load

For Plan 162 prove:

- known-length stream byte accounting under concurrency;
- unknown-length HTTP/1 framing over keep-alive;
- HEAD/body-forbidden streams are never polled;
- producer error before/after commitment follows documented behavior;
- client disconnect cancels producer;
- slow reader bounds producer advancement/RSS;
- long continuously progressing stream is not killed by no-progress timeout;
- stalled reader triggers Plan 164 write timeout and releases service/connection permits;
- shutdown drops/cancels streams deterministically.

Run allocation/RSS comparison against equivalent buffered responses to demonstrate why streaming exists.

# Track B — Transport-neutral parity

For Plan 163 create a deterministic caller-owned-stream harness.

Preferred structure:

- `tokio::io::duplex` for basic tests;
- a small test-only `AsyncRead + AsyncWrite` wrapper that can inject latency, bandwidth limits, short reads/writes, stalls, EOF, and write failures;
- shared runtime/admission state across many independent streams.

Do not introduce an I2P dependency into EggServe tests.

Prove:

- equivalent HTTP bytes produce equivalent canonical behavior over TCP and test transport;
- missing socket endpoints do not break services or generate fake addresses;
- latency/bandwidth shaping exercises timeouts/backpressure correctly;
- shutdown/cancellation/failure releases permits;
- Plan 165 response policy is identical over TCP/TLS/caller-owned streams.

This is the qualification proxy for an I2P router handing EggServe an established streaming transport.

# Track C — Parser/admission hostile-load tests

For Plan 164 cover:

- header count at/beyond configured limit;
- parser/header byte buffer at/beyond limit;
- aggregate canonical header byte limit;
- request target at/beyond limit;
- slowloris within/beyond header deadline;
- malformed/duplicate Content-Length and TE/CL corpus after Hyper refresh;
- many idle keep-alive clients without consuming service permits;
- service permit saturation while connections remain healthy;
- connection permit saturation;
- repeated saturation/recovery cycles;
- max requests per connection;
- idle timeout vs continuously active connections;
- optional hard lifetime;
- stalled writer/no-progress timeout;
- fd/handle/task/RSS trend after repeated failure cycles.

Re-run existing smuggling/canonical-wire fuzz/corpus suites with the refreshed Hyper version.

# Track D — Python low-level qualification

For Plan 166 measure and verify separately:

- low-level buffered callback throughput/latency;
- low-level streaming response throughput/latency;
- request body iterator backpressure;
- callback semaphore saturation;
- generic service semaphore saturation;
- GIL behavior under concurrent Rust network I/O;
- callback exceptions and iterator exceptions;
- long-running callback after HTTP timeout does not corrupt/reuse request state;
- repeated start/stop and shutdown under callback load;
- Python object/thread/RSS trend;
- stdlib facade regression suite in the same build.

Do not compare Python callback throughput to native Rust static throughput as though they are equivalent workloads.

# Track E — Privacy/fingerprint golden evidence

Capture raw HTTP/1 response fixtures for runtime-generated success/error responses in normal and anonymity-sensitive profiles.

Assertions:

- normal profile contains exactly one standards-compliant Date and no Server by default;
- custom Date provider yields expected HTTP-date;
- explicit Date suppression yields no Date from either EggServe or Hyper;
- denylisted response headers never survive finalization;
- no runtime-generated response contains EggServe/Hyper/Rust/Python/OS/build-path/version identifiers;
- canonical errors remain fixed/bounded and do not contain exception text;
- static validator/timestamp policy matches the selected profile;
- HEAD variants preserve correct metadata without bodies;
- TCP/TLS/non-socket fixtures match except for transport-relevant semantics.

Maintain a documented threat statement: these tests prove absence of selected gratuitous identifiers, not inability to fingerprint EggServe statistically.

# Track F — Soak and failure recovery

Run sustained tests long enough to expose lifecycle drift:

- steady keep-alive traffic;
- high connection churn;
- alternating saturation/recovery;
- streaming uploads/downloads;
- slow clients;
- disconnect churn;
- TLS handshake churn;
- repeated shutdown/start in test processes;
- caller-owned stream creation/drop churn;
- Python callback/stream churn.

Track:

- RSS slope;
- fd/handle slope;
- task/thread count;
- semaphore available permits;
- file handles;
- error/log rate;
- CPU when idle and under stalled-client scenarios.

Any monotonic resource growth requires root cause before release qualification.

# Track G — Optional CGI/FastCGI evidence

Only if Plan 167 implements adapters:

### CGI

- child-process concurrency and reaping;
- timeout/disconnect kill/reap;
- stdout/stderr bounds;
- environment/input sanitization;
- normal historical compatibility fixtures;
- no shell/request injection path;
- no zombie/fd growth soak.

### FastCGI

- fragmented/malformed record corpus;
- Responder request/response mapping;
- large streaming STDIN/STDOUT with backpressure;
- STDERR bounds;
- backend timeout/disconnect/abort;
- no cross-request contamination;
- 502/504 generic error mapping;
- backend connection/resource recovery.

Do not make core release claims depend on an optional adapter's benchmark superiority.

## Platforms

Preserve the existing supported platform qualification policy. At minimum run core correctness on the normal CI matrix and performance evidence on representative:

- Linux x86_64;
- Linux aarch64 where available/relevant to SBC/router deployment;
- macOS arm64;
- Windows x86_64 for supported runtime/Python behavior.

Performance numbers are platform-specific. Do not combine them into one average.

## Documentation/release outputs

At closure produce/update:

- benchmark methodology and machine-readable results location;
- deployment-profile documentation;
- timeout/resource-limit reference;
- Rust embedding example using caller-owned stream/service API;
- Python low-level service example;
- anonymity-sensitive threat model and exact response-policy knobs;
- support matrix for optional adapters;
- README wording limited to evidence-backed claims;
- `.opencode/skills/eggserve-dev/SKILL.md` and `AGENTS.md` stable invariants.

## Acceptance criteria

- [ ] All existing conformance/corpus/fuzz/static/Python suites remain green after Plans 162–166.
- [ ] New streaming responses have bounded memory/backpressure and correct HTTP/1 framing under concurrency and failure.
- [ ] The canonical connection driver is qualified over a fault-injectable non-socket transport with TCP parity.
- [ ] Parser, connection, service, keep-alive, request-count, and stalled-write limits have deterministic tests and resource-recovery evidence.
- [ ] High-concurrency benchmarks report reproducible latency/throughput/resource data for native and Python paths without unsupported comparisons.
- [ ] Soak testing shows no unexplained monotonic task/thread/fd/handle/RSS growth.
- [ ] Privacy golden tests prove configured Server/Date/header/error/static-metadata behavior and no known runtime version strings.
- [ ] Documentation states the separate-WAF assumption for the embedded anonymity-sensitive/I2P use case.
- [ ] Release claims remain profile-specific and do not imply a general edge proxy/app-server/WAF product.

## Closure rule

Do not mark the revised roadmap complete merely because the APIs compile. Plan 161 closes only after this evidence is recorded and documentation is updated to match the qualified behavior.
