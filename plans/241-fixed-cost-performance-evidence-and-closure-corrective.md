# Plan 241 — Fixed-cost performance evidence and closure corrective

## Purpose

Close the evidence gaps left by Plans 234–240 without reopening the retained
production optimizations.

The post-Plan-240 review found that the implementation itself is disciplined and
the current closing SHA has successful remote CI, but the retained performance
record is narrower than the acceptance matrix written into Plans 234 and 240.
The existing closure proves the nine native static size/concurrency cases and
the deterministic correctness/resource suite. It does **not** retain the full
candidate-specific A/B evidence that Plan 240 required for custom-service,
path-specific, TLS, and Python callback/slow-stream workloads.

This is an evidence/documentation corrective. It must not change production
code, defaults, dependencies, public Rust/Python APIs, protocol tiers, or
runtime architecture.

If any reproduction exposes a correctness/security regression or a material,
repeatable performance/resource regression caused by a retained optimization,
stop this plan and open a separate narrowly scoped production corrective. Do
not repair production behavior under Plan 241.

## Starting state

Planning review baseline:

- Plan 234/240 comparison baseline:
  `504c3d31f46399d361e57a5bab51a6325a0f4acd`
- retained production candidate:
  `af9727870236e684746858bc32eb37aa22892251`
- documentation/closure SHA:
  `5b048cbf66f57957625c9ad8b658635a56ac9593`
- current closure evidence:
  `benchmarks/234-fixed-cost-baseline/`
  and `benchmarks/240-fixed-cost-closure/`
- current remote CI evidence for the closing SHA:
  GitHub Actions run `35538302042`, successful for
  `5b048cbf66f57957625c9ad8b658635a56ac9593`

The implementation commit and documentation-only closing commit have the same
production behavior. Performance comparisons should use the frozen baseline
and production candidate SHAs above. Repository/CI provenance should also
record the later documentation/evidence SHAs distinctly.

## Evidence layout

Create:

```text
benchmarks/241-fixed-cost-evidence-corrective/
  README.md
  results.json
  decisions.md
  raw/
    native-custom/
    native-path/
    tls/
    python-callback/
    python-stream/
  ci/
```

Retain compact per-trial machine-readable results. Do not keep only aggregate
medians or prose summaries.

Every result set must record:

- baseline and candidate SHA;
- root and Python lockfile hashes where applicable;
- exact build command/profile/features;
- compiler/tool versions;
- OS/architecture/CPU/logical CPU/memory;
- runtime limits and workload configuration;
- warm-up policy and number of measured trials;
- throughput/latency/resource/error fields relevant to the workload;
- whether the result is measured, source-mechanical, or unavailable.

## Track A — complete the native H1 matrix

Plan 240 required more than the nine static size/concurrency points already
retained. Re-run baseline and candidate on the same host, alternating order
where practical, with one excluded warm-up and at least three measured trials.

### A1. Custom-service small response

Measure the direct canonical custom-service path with a 1 KiB bytes response:

- concurrency 1;
- concurrency 16;
- concurrency 64.

This is the workload most directly related to the Plan 237 dispatch cleanup and
must not be inferred from static-file results.

### A2. Static response-shape cases

Add representative cases for:

- HEAD of a known-length static file;
- conditional 304;
- one satisfiable range request;
- one-component short path;
- nested path;
- query-bearing request target;
- percent-encoded safe path.

Use the existing native harness infrastructure where possible. Extend the
benchmark harness only in benchmark-only code.

Record status/body/header correctness alongside timing so a fast incorrect
response cannot qualify.

### A3. Caller-owned H1 sanity

Run the existing caller-owned connection path against baseline and candidate.
A compact smoke/performance sanity result is sufficient; this is not intended
to become a new throughput benchmark.

## Track B — candidate-specific TLS closure

Plan 234 required established-TLS metadata evidence, and Plan 240 required a
baseline/candidate TLS comparison. Capture both now.

Use long-lived TLS connections to separate per-request work from handshake
cost.

Measure at minimum:

1. established TLS H1, 1 KiB response, metadata-light configuration;
2. established TLS H1 with the normal exposed TLS metadata used by request
   construction;
3. opt-in peer-certificate-chain exposure when the current test fixtures make
   it practical and deterministic;
4. representative handshake churn only as a regression check, not as evidence
   for per-request metadata optimization.

For established sessions record:

- RPS/throughput;
- p50/p95/p99;
- CPU where available;
- RSS;
- errors/timeouts;
- connection reuse;
- TLS feature/profile/configuration;
- whether peer-chain exposure was enabled.

The Plan 237 metadata-sharing subtrack remains DEFER. Plan 241 does not reopen
it merely because TLS is being measured.

## Track C — Python callback compatibility/performance closure

Build and install the actual wheel for both frozen baseline and candidate in
isolated environments. Do not substitute direct Rust extension tests for the
installed-wheel surface.

Measure:

- trivial handler returning an empty response;
- trivial handler returning a small bytes response;
- handler reading only `request.method`;
- handler reading `request.headers`;
- handler reading `request.header_items`;
- handler reading raw target/path/query byte views;
- handler reading byte header items;
- metadata-heavy handler reading address/TLS/proxy/effective fields available
  in the selected fixture.

The goal is not to establish a universal Python throughput claim. It is to
verify that Plan 239's lazy compatibility views actually avoid eager work for
minimal handlers without making accessed views materially worse or changing
their values.

Record:

- requests/s or elapsed/request where stable;
- p50/p95/p99 when the harness supports it;
- RSS;
- thread count;
- errors;
- returned values/behavior checks;
- wheel identity and Python version.

If no supported allocator counter exists, retain the same limitation recorded
by Plan 234 and make only source-mechanical allocation claims.

## Track D — Python slow-stream resource matrix

The Plan 239 producer-thread redesign is DEFER, but Plan 234 explicitly required
resource evidence for the current one-thread-per-stream design. Capture that
evidence without changing the architecture.

Exercise synchronous `Response.stream` with deliberately slow readers at:

- 10 concurrent active streams;
- 100 concurrent active streams;
- one larger `N` selected from host capacity that is high enough to expose
  resource scaling without destabilizing the qualification host.

For each point record:

- process thread count before/during/after;
- RSS before/peak/after;
- open fd count;
- callback semaphore behavior if observable through existing diagnostics;
- channel/backpressure behavior;
- disconnect cleanup;
- time to return to steady-state thread/RSS counts after clients close;
- graceful shutdown/drain completion;
- errors/truncation/timeouts.

Also run at least one disconnect-while-backpressured case and one shutdown with
active streams.

The expected result may legitimately show approximately one producer thread per
active synchronous stream. That is not itself a failure: the current design is
an intentional isolation/backpressure tradeoff. The purpose is to document its
resource envelope truthfully.

If resource growth is unexpectedly unbounded beyond the known per-stream
thread/channel model, or cleanup fails to return resources, stop and open a
separate production corrective.

## Track E — Unix resolver syscall proof

Plan 236's root-FD change is mechanically clear, but the closure should retain a
direct before/after syscall record rather than relying only on source review.

For baseline and candidate on Linux capture a focused hardened static request
for:

- one-component file;
- nested file;
- root directory/resource case if the current harness supports it cleanly.

Confirm:

- the candidate ordinary non-root path removes the root descriptor-duplication
  syscall/close pair;
- security-required `statat(AT_SYMLINK_NOFOLLOW)`, `openat(O_NOFOLLOW)`,
  post-open metadata/type validation, and close behavior remain;
- nested traversal still owns and closes intermediate descriptors correctly.

Do not optimize further under this track.

## Track F — reconcile deterministic qualification

Re-run the current routine matrix on the evidence implementation SHA:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
bash scripts/test-python-wheel.sh
```

Also run the targeted suites relevant to the retained optimizations:

- request-target parsing and byte-fidelity tests;
- request-body/trailer/lifecycle tests;
- static confinement/symlink/special-file regressions;
- direct-H1 and compatibility-H1 parity;
- tunnel lifecycle/drain tests;
- trusted proxy/TLS metadata tests;
- Python request-property tests;
- Python stream disconnect/shutdown tests.

No new absolute timing gate belongs in CI.

## Track G — remote CI provenance

The repository already has a successful push CI run for the current Plan 240
closing SHA:

```text
SHA: 5b048cbf66f57957625c9ad8b658635a56ac9593
run: 35538302042
result: success
jobs: rust, supply-chain, python
```

Record that run in the Plan 241 evidence.

After committing the Plan 241 evidence artifacts/documentation, require a
successful remote CI run for that exact evidence SHA as well. Record the run ID,
URL, timestamp, job conclusions, and exact SHA.

If a final metadata-only commit is needed solely to record the evidence-SHA CI
run, distinguish:

- production candidate SHA;
- evidence-content SHA verified by CI;
- final metadata-record SHA.

Do not describe a later metadata-only SHA as though its code was independently
performance benchmarked.

## Track H — acceptance reconciliation

Update the historical evidence record without rewriting history.

### Plan 234

Add a short executed-result note identifying which originally requested
baseline measurements were only completed by Plan 241. Do not falsely claim
they existed at Plan 234 execution time.

### Plan 240

Reconcile each acceptance criterion with one of:

- `PASS — Plan 240 evidence`;
- `PASS — completed by Plan 241 corrective`;
- `N/A — subtrack deliberately NO-GO/DEFER`;
- `BLOCKED` with an explicit reason.

The final record must explicitly cover:

- custom H1;
- path-specific static cases;
- TLS;
- Python callback;
- Python slow-stream resource behavior;
- syscall/resource effects;
- routine/package/supply-chain/wheel checks;
- remote CI.

### Durable docs

Update only documents that make current evidence claims:

- `benchmarks/README.md`;
- `architecture/testing-and-conformance.md`;
- `plans/ROADMAP.md`;
- `AGENTS.md` and the EggServe development skill only if their evidence index
  would otherwise be stale.

Do not change architecture/product descriptions unless the evidence actually
changes a currently documented fact.

## Decision policy

Plan 241 is evidence-only.

For every newly measured workload, classify the result:

```text
workload:
baseline:
candidate:
resource/error result:
interpretation:
decision: CONFIRMS | NEUTRAL | CONTRADICTS
```

- `CONFIRMS`: candidate behaves equivalently or better and supports the
  retained simplification.
- `NEUTRAL`: variance dominates, but correctness/resource behavior is
  unchanged; mechanically simpler fixed-cost removal remains valid.
- `CONTRADICTS`: repeatable material regression or incorrect resource
  behavior attributable to the retained change.

Any `CONTRADICTS` result that affects production behavior blocks Plan 241
closure and requires a separate corrective implementation plan.

Do not silently weaken the workload or acceptance language to convert a
contradiction into a pass.

## Explicit non-goals

Plan 241 does not authorize:

- production Rust/Python source changes;
- reopening Plan 238;
- redesigning the Python stream producer;
- connection-metadata sharing;
- new caches;
- sendfile/splice/io_uring;
- mmap;
- custom allocators;
- global buffer pools;
- new dependencies;
- API changes;
- H2/H3 tier changes;
- file-stream chunk-size retuning.

## Completion criteria

Plan 241 is complete only when:

- [ ] baseline/candidate custom 1 KiB c1/c16/c64 evidence is retained;
- [ ] HEAD, 304, range, short/nested/query/encoded path cases are retained;
- [ ] established TLS baseline/candidate evidence is retained;
- [ ] Python installed-wheel callback evidence covers lazy and accessed views;
- [ ] Python 10/100/N slow-stream thread/RSS/cleanup/shutdown evidence is
      retained;
- [ ] before/after Unix resolver syscall evidence directly demonstrates the
      removed root-FD duplication while security checks remain;
- [ ] all newly measured workloads are classified
      CONFIRMS/NEUTRAL/CONTRADICTS;
- [ ] the routine/package/supply-chain/Python-wheel matrix passes;
- [ ] successful CI run `35538302042` is recorded for the Plan 240 closure SHA;
- [ ] successful remote CI is recorded for the Plan 241 evidence-content SHA;
- [ ] Plan 234 and Plan 240 acceptance records are reconciled truthfully;
- [ ] durable benchmark/testing/roadmap indexes point to the new artifacts;
- [ ] no production code/default/dependency/API/tier change lands under this
      plan.

## Handoff

This should be the final evidence corrective for Plans 234–240 if the expanded
matrix confirms or is neutral on the retained implementation.

If the matrix contradicts a retained optimization, stop at the evidence commit,
mark the relevant criterion BLOCKED, and write a new production corrective plan
that names the exact workload/regression. Do not extend Plan 241 into an
implementation campaign.
