# Testing and Conformance — Deep Dive

eggserve uses a multi-layered testing strategy: Rust unit/integration tests, Python test suites, shared conformance corpora, fuzzing, and live HTTP wire tests.

## Test Layers

| Layer | Location | Scope | Count |
|-------|----------|-------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic | current suite |
| Rust integration tests | `crates/eggserve-core/tests/*.rs` | Cross-module, live TCP, TLS | 30 files |
| Rust bin tests | `crates/eggserve-bin/tests/*.rs` | Production binary paths | 4 files |
| Python native primitives | `crates/eggserve-python/tests/test_primitives.py` | PyO3 bindings and canonical types | current suite |
| Python server façade | `crates/eggserve-python/tests/test_https_server_compat.py`, `test_http_server_compat.py`, `test_simple_http_handler_compat.py` | HTTP server compatibility, TLS, and policy behavior | current suite |
| Python subprocess API | `crates/eggserve-python/tests/test_server.py` | CLI subprocess lifecycle | current suite |
| Python server primitives | `crates/eggserve-python/tests/test_server_primitives.py` | Server primitive bindings | current suite |
| Python server integration | `crates/eggserve-python/tests/test_server_integration.py` | Live concurrency and shutdown | current suite |
| Python canonical conformance | `crates/eggserve-python/tests/test_canonical_conformance.py` | Rust/Python parity | current suite |
| Python canonical request types | `crates/eggserve-python/tests/test_canonical_request_types.py` | Request type correctness | current suite |
| Python body primitives | `crates/eggserve-python/tests/test_body_primitives.py` | Body consumption | current suite |
| Python body conformance | `crates/eggserve-python/tests/test_body_conformance.py` | Body corpus parity | current suite |
| Python body wire | `crates/eggserve-python/tests/test_body_wire.py` | Wire-level body tests | current suite |
| Python boundary hardening | `crates/eggserve-python/tests/test_boundary_hardening.py` | Security hardening and namespace boundaries | current suite |
| Python public API | `crates/eggserve-python/tests/test_public_api.py` | Supported namespace and demotion checks | focused |
| Python parity matrix | `crates/eggserve-python/tests/test_parity_matrix.py` | Real-socket Rust/Python parity | current suite |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing | 11 targets |
| Conformance corpus | `conformance/*.json` | Shared Rust/Python test data | 2 corpora |
| Executable examples | `examples/`, `crates/eggserve-core/examples/` | Canonical CLI/Python/Rust product demonstrations | current |

The installed-wheel script is the authoritative Python test entry point; its count changes with the compatibility façade and is intentionally not duplicated here.

`./scripts/verify.sh full` additionally runs `cargo check -p eggserve-core
--examples`, builds those examples for the live harness, then starts the
canonical Python static/custom examples and Rust static/custom examples on
loopback port `0`. Each smoke test makes a real HTTP request and verifies clean
shutdown; the process harness uses only Python's standard library.

## Rust Integration Test Files

| File | Feature Gate | Focus |
|------|-------------|-------|
| `integration.rs` | — | Method validation, body rejection, conditional/range requests, HEAD parity |
| `http_wire_correctness.rs` | — | Raw TCP wire tests: GET/HEAD/POST/404/403/400/413/206/416/304 |
| `http_primitives_integration.rs` | — | 15 live TCP tests through hyper client/server stack |
| `canonical_conformance.rs` | — | Canonical HTTP type conformance: Method, HttpVersion, HeaderBlock, StatusCode, Response normalization |
| `canonical_wire_interop.rs` | — | Wire-level canonical type interop |
| `corpus_replay.rs` | — | Replays fuzz seed corpora to catch regressions |
| `body_conformance.rs` | — | Body policy selection, empty/fixed-length/over-limit/chunked bodies |
| `body_primitives.rs` | — | RequestBody read/chunk/one-shot/error taxonomy |
| `request_body_integration.rs` | — | Full body ingestion pipeline: policy, limit, timeout, accounting |
| `request_body_wire.rs` | — | Wire-level body tests: fixed-length, chunked, over-limit, method rejection |
| `request_body_timeout_interaction.rs` | — | Body timeout + handler timeout interaction |
| `request_body_cancellation.rs` | — | Body cancellation and disconnect handling |
| `request_body_tls.rs` | `tls` | Body handling over TLS connections |
| `body_properties.rs` | — | BodySource properties and invariants |
| `tls_service_parity.rs` | `tls` | TLS + non-TLS behavioral parity |
| `server_integration.rs` | — | Server lifecycle, Service trait, StaticService |
| `lifecycle_integration.rs` | — | Lifecycle state machine: Created→Running→Draining→Stopped |
| `public_api_consumers.rs` | — | Validates public API surface |
| `api_stability.rs` | — | API stability snapshot checks |
| `no_hyper_in_public_api.rs` | — | Ensures no Hyper types leak into public API |
| `production_path.rs` (bin) | — | Binary production path validation |
| `cli_validation.rs` (bin) | — | CLI argument validation |
| `tls_abuse.rs` (bin) | `tls` | TLS error handling and abuse resistance |
| `logging_modes.rs` (bin) | — | CLI logging mode verification: default text, JSON, quiet, none |
| `fault_injection.rs` | — | Fault injection for filesystem and I/O error paths |
| `filesystem_race_qualification.rs` | — | Filesystem race condition qualification |
| `ops_integration.rs` | — | Structured logging integration |
| `qualification.rs` | — | Qualification test harness |
| `stateful_fuzz_replay.rs` | — | Stateful fuzz corpus replay |
| `unix_validator_qualification.rs` | — | Unix path validator qualification |
| `windows_feasibility.rs` | `windows-adversarial-qualification` | Windows feasibility spike |
| `windows_plan084.rs` | `windows-adversarial-qualification` | Windows handle-relative directory retention |
| `windows_plan086.rs` | `windows-adversarial-qualification` | Windows adversarial filesystem qualification |
| `streaming_buffer_qualification.rs` | — | Exact range boundaries, chunk-crossing, buffer isolation, zero-length files, client disconnect release, forced shutdown release, concurrent exhaustion (503), HEAD non-acquisition, configurable chunk sizes |

## Conformance Corpora

### `conformance/corpus.json`

Normative conformance corpus for canonical HTTP types. Groups:
- **Methods**: GET, HEAD, POST, PUT, DELETE, PATCH + extension methods with expected `as_str`, `is_safe`, `is_idempotent`, `permits_static`
- **Status codes**: expected classification (informational, success, redirect, client-error, server-error)
- **Headers**: name validation, value constraints
- **Versions**: HTTP/1.0, HTTP/1.1 parsing

Consumed by both Rust (`tests/canonical_conformance.rs`) and Python (`test_canonical_conformance.py`).

### `conformance/body_corpus.json`

Shared Rust/Python conformance corpus for request body integration. Groups:
- **body_policy_selection**: reject/buffer/stream policies with expected status, handler_called, body presence
- **fixed/chunked length accounting**: byte-accurate body size tracking
- **limit enforcement**: oversized bodies → 413
- **one-shot consumption**: second read raises error
- **service-defined method bodies**: GET/HEAD/DELETE/OPTIONS/extension bodies
  are accepted when the service declares Buffer/Stream; static serving declares
  Reject
- **partial consumption**: incomplete body → Close policy

Consumed by both Rust (`tests/body_conformance.rs`) and Python (`test_body_conformance.py`).

Runtime corrective coverage also proves canonical static file/range variants,
single-server file-stream saturation, rootless custom startup, authoritative
`Server` headers, and wire-visible close behavior after partial stream-body
consumption. Release smoke tests use a temporary fixture file rather than the
repository root.

## Fuzzing

### Fuzz Targets (11)

| Target | What it fuzzes |
|--------|---------------|
| `request_target` | HTTP origin-form parsing, path confinement, request target validation, request head construction |
| `percent_decode` | Single-pass percent decoding |
| `path_components` | Path normalization and component validation |
| `validate_method` | HTTP method construction and validation, body rejection for read-only methods |
| `range_header` | Range header parsing |
| `if_none_match` | If-None-Match ETag comparison |
| `platform_component` | Windows platform-specific checks |
| `fuzz_header_block` | HeaderName, HeaderValue, and HeaderBlock operations |
| `fuzz_normalize_response` | StatusCode validation, response building, response normalization, Content-Length reconciliation |
| `fuzz_request_body` | RequestBody state machine |
| `fuzz_directory_buffer` | Directory listing buffer behavior |

### Seed Corpora

11 corpus directories under `fuzz/corpus/` providing initial inputs for each fuzz target. Coverage includes canonical HTTP types, response normalization, request body, header operations, method validation, and content-length reconciliation.

### CI Integration

- **Property tests** run in normal `cargo test` (assertions on arbitrary input)
- **Corpus regression replay** via `cargo test -p eggserve-core --test corpus_replay`

### Fuzzing Invariants

- No panics on arbitrary input
- No `..` or `.` in accepted path components
- No NUL bytes in decoded paths
- No double-decoding of percent-encoded sequences
- Satisfiable ranges always fall within file size
- All rejection reasons map to valid `PathRejection` variants

## Test Execution

### Rust tests

```sh
cargo test --workspace                                        # all unit + integration
cargo test -p eggserve-core --test http_wire_correctness      # raw wire tests
cargo test -p eggserve-core --test canonical_conformance      # canonical type conformance
cargo test -p eggserve-core --test corpus_replay              # fuzz corpus replay
cargo test -p eggserve-bin --features tls                     # TLS tests
cargo test -p eggserve-bin --test production_path             # production path tests
```

### Python tests

The authoritative Python test entry point is `scripts/test-python-wheel.sh` (builds CLI, builds wheel, installs in venv, runs tests). Manual test commands:

```sh
cd crates/eggserve-python
PYTHONPATH=python python -m unittest eggserve.test_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_server -v
PYTHONPATH=python python -m unittest eggserve.test_server_integration -v
PYTHONPATH=python python -m unittest eggserve.test_canonical_conformance -v
PYTHONPATH=python python -m unittest eggserve.test_canonical_request_types -v
PYTHONPATH=python python -m unittest eggserve.test_body_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_body_conformance -v
PYTHONPATH=python python -m unittest eggserve.test_body_wire -v
PYTHONPATH=python python -m unittest eggserve.test_boundary_hardening -v
PYTHONPATH=python python -m unittest discover -s crates/eggserve-python/tests -p 'test_*.py' -v
PYTHONPATH=python python -m unittest eggserve.test_parity_matrix -v
```

### Packaging smoke tests

```sh
cd crates/eggserve-python/packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

### Platform and external-consumer qualification

Routine CI is intentionally Linux-focused. The product-level qualification
matrix covers the standalone CLI, installed wheel, Python static/custom
facades, and an external Rust consumer. The manual
`.github/workflows/platform-qualification.yml` workflow runs installed-wheel
checks on macOS arm64 and Windows filesystem qualification suites; it is not
part of every push/PR.

## Plan 168 qualification track mapping

Plan 168 is a qualification phase, not a feature phase. Each track is
evidenced by deterministic suites (not absolute-timing gates):

| Track | Evidence |
|-------|----------|
| A — Streaming correctness under load | `tests/response_streaming.rs` (framing, `Send + !Sync` producer acceptance, HEAD/body-forbidden never poll, mismatch teardown, panic containment, cancellation, keep-alive reuse), `tests/streaming_buffer_qualification.rs` (range boundaries, disconnect/shutdown permit release, 503 exhaustion, HEAD non-acquisition), `tests/request_body_*` (ingestion, timeouts, cancellation) |
| B — Transport-neutral parity | `tests/transport_driver.rs` (duplex driver, TCP parity, no fabricated addresses), `tests/production_controls.rs` (duplex admission/timeout shaping), example `caller_owned_stream.rs` |
| C — Parser/admission hostile load | `tests/http_wire_correctness.rs` (raw wire: smuggling corpus, framing ambiguity, lifecycle), `tests/production_controls.rs` (limits, saturation/recovery, idle/write/total deadlines), `tests/fault_injection.rs`, `tests/stateful_fuzz_replay.rs`, `tests/corpus_replay.rs` |
| D — Python low-level qualification | `crates/eggserve-python/tests/test_lowlevel_runtime.py` (buffered/streaming throughput paths, backpressure, saturation, GIL behavior, exceptions, shutdown churn), example `examples/python_lowlevel_service.py` |
| E — Privacy/fingerprint goldens | `tests/response_privacy.rs` (Server/Date/denylist/error/static-metadata behavior over TCP/TLS/non-socket, no version strings); threat statement: absence of selected gratuitous identifiers, not un-fingerprintability |
| F — Soak and failure recovery | `tests/soak/` (repo-level), repeated saturation/recovery cycles in `production_controls.rs`, TLS churn in `tls_abuse.rs`/`request_body_tls.rs` |
| G — CGI/FastCGI | Closed as no-go (Plan 167): no in-tree adapters, no adapter evidence owed; downstream matrix in `docs/extension-contract.md` |

Performance snapshots and the regression/claims policy live in
`benchmarks/README.md`; machine-readable results in
`benchmarks/088-baseline/results.json`, `benchmarks/168-qualification/results.json`,
and `benchmarks/170-closure/results.json`.

## Plan 170 performance-evidence closure

Plan 170 is manual same-machine evidence, never an absolute-timing CI gate.
The standard-library harness at `benchmarks/170-closure/benchmark.py` records
the final source/lockfile identity, explicit runtime limits, machine metadata,
warm-up/trial discipline, latency percentiles, throughput, errors, CPU/RSS,
fd/thread observations, and variance for these workload families:

| Family | Coverage |
|--------|----------|
| Native static HTTP/1 | 1 KiB, 128 KiB, and 1 MiB at 1/16/64/256 keep-alive workers |
| Native custom service | 1 KiB buffered, 1 MiB known/unknown streams, and 16 MiB streaming/backpressure |
| Installed Python low-level | 1 KiB buffered, 1 MiB bounded stream, callback admission, and recovery after saturation |
| Caller-owned driver | Ignored Rust microbenchmark over `tokio::io::duplex`, buffered and streamed; no network score |
| TLS | Established keep-alive 1 KiB/1 MiB and separate new-connection handshake churn |
| Substitution baseline | Same-session CPython `python -m http.server`, 1 KiB/1 MiB at 1/16/64 workers |

The evidence supports only profile-specific statements about representative
scaling, bounded streaming, the low-level substrate, embedding overhead, TLS
overhead, and static migration behavior. It does not qualify arm64
performance when no arm64 host was available and does not establish edge
server parity, DDoS resistance, anonymity, or universal superiority.

## See Also

- [overview.md](overview.md) — Architecture overview
- [eggserve-core.md](eggserve-core.md) — Core library modules under test
- [eggserve-python.md](eggserve-python.md) — Python test suites
- [release-process.md](../docs/release-process.md) — Manual release procedure
- [../docs/fuzzing.md](../docs/fuzzing.md) — Fuzzing documentation
