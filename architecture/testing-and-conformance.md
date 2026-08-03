# Testing and Conformance — Deep Dive

eggserve uses a multi-layered testing strategy: Rust unit/integration tests, Python test suites, shared conformance corpora, fuzzing, and live HTTP wire tests.

## Test Layers

| Layer | Location | Scope | Count |
|-------|----------|-------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic | current suite |
| Rust integration tests | `crates/eggserve-core/tests/*.rs` | Cross-module, live TCP, TLS | 24 files |
| Rust bin tests | `crates/eggserve-bin/tests/*.rs` | Production binary paths | 1 file |
| Python native primitives | `python/eggserve/test_primitives.py` | PyO3 bindings, 143 tests | 143 |
| Python server façade | `crates/eggserve-python/tests/test_https_server_compat.py` and façade tests | HTTP server compatibility, TLS, and policy behavior | current suite |
| Python native primitives | `crates/eggserve-python/tests/test_primitives.py` and focused suites | PyO3 bindings and canonical types | current suite |
| Python subprocess API | `crates/eggserve-python/tests/test_server.py` | CLI subprocess lifecycle | current suite |
| Python server integration | `crates/eggserve-python/tests/test_server_integration.py` | Live concurrency and shutdown | current suite |
| Python canonical conformance | `crates/eggserve-python/tests/test_canonical_conformance.py` | Rust/Python parity | current suite |
| Python canonical request types | `crates/eggserve-python/tests/test_canonical_request_types.py` | Request type correctness | current suite |
| Python body primitives | `crates/eggserve-python/tests/test_body_primitives.py` | Body consumption | current suite |
| Python body conformance | `crates/eggserve-python/tests/test_body_conformance.py` | Body corpus parity | current suite |
| Python body wire | `crates/eggserve-python/tests/test_body_wire.py` | Wire-level body tests | current suite |
| Python boundary hardening | `crates/eggserve-python/tests/test_boundary_hardening.py` | Security hardening and namespace boundaries | current suite |
| Python public API | `crates/eggserve-python/tests/test_public_api.py` | Supported namespace and demotion checks | focused |
| Python parity matrix | `crates/eggserve-python/tests/test_parity_matrix.py` | Real-socket Rust/Python parity | current suite |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing | 19 targets |
| Conformance corpus | `conformance/*.json` | Shared Rust/Python test data | 2 corpora |

The installed-wheel script is the authoritative Python test entry point; its count changes with the compatibility façade and is intentionally not duplicated here.

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
| `request_body_tls.rs` | `client-tls` | Body handling over TLS connections |
| `body_properties.rs` | — | BodySource properties and invariants |
| `client_integration.rs` | `client` | 23 tests: GET/HEAD/POST/PUT/DELETE/PATCH, timeouts, TLS, validation |
| `client_interop.rs` | `client` | 48 tests: edge cases, chunked bodies, duplicate headers, premature EOF |
| `client_tls.rs` | `client-tls` | 7 tests: TLS verification, self-signed certs, verify_tls bypass |
| `tls_service_parity.rs` | `tls` | TLS + non-TLS behavioral parity |
| `server_integration.rs` | — | Server lifecycle, Service trait, StaticService |
| `lifecycle_integration.rs` | — | Lifecycle state machine: Created→Running→Draining→Stopped |
| `public_api_consumers.rs` | — | Validates public API surface |
| `api_stability.rs` | — | API stability snapshot checks |
| `no_hyper_in_public_api.rs` | — | Ensures no Hyper types leak into public API |
| `production_path.rs` (bin) | — | Binary production path validation |
| `streaming_buffer_qualification.rs` | — | Plan 088: exact range boundaries, chunk-crossing, buffer isolation, zero-length files, client disconnect release, forced shutdown release, concurrent exhaustion (503), HEAD non-acquisition, configurable chunk sizes |

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
- **GET-with-body rejection**: bodies on GET/HEAD rejected
- **partial consumption**: incomplete body → Close policy

Consumed by both Rust (`tests/body_conformance.rs`) and Python (`test_body_conformance.py`).

## Fuzzing

### Fuzz Targets (19)

| Target | What it fuzzes |
|--------|---------------|
| `request_target` | HTTP origin-form parsing |
| `percent_decode` | Single-pass percent decoding |
| `path_components` | Path normalization and component validation |
| `validate_request_target` | Full request target validation pipeline |
| `validate_method` | HTTP method validation |
| `url_parse` | Client URL parsing |
| `range_header` | Range header parsing |
| `if_none_match` | If-None-Match ETag comparison |
| `platform_component` | Windows platform-specific checks |
| `fuzz_method` | Canonical Method construction |
| `fuzz_status_code` | StatusCode validation |
| `fuzz_header_block` | HeaderBlock operations |
| `fuzz_header_name` | HeaderName validation |
| `fuzz_header_value` | HeaderValue validation |
| `fuzz_normalize_response` | Response normalization |
| `fuzz_request_body` | RequestBody state machine |
| `fuzz_request_head` | RequestHead construction |
| `fuzz_response_builder` | Response builder validation |
| `fuzz_content_length_reconciliation` | Content-Length consistency |

### Seed Corpora

19 corpus directories under `fuzz/corpus/` providing initial inputs for each fuzz target. Coverage includes canonical HTTP types, response normalization, request body, header operations, method validation, status codes, and content-length reconciliation.

### CI Integration

- **Property tests** run in normal `cargo test` (assertions on arbitrary input)
- **Weekly scheduled fuzz runs** (60s per target) via `.github/workflows/fuzz.yml`
- **Corpus regression replay** on every PR/push via `.github/workflows/fuzz-replay.yml`

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
cargo test -p eggserve-core --features client                 # client feature tests
cargo test -p eggserve-bin --features tls                     # TLS tests
cargo test -p eggserve-bin --test production_path             # production path tests
```

### Python tests

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
PYTHONPATH=python python -m unittest eggserve.test_api_consumers -v
PYTHONPATH=python python -m unittest eggserve.test_api_stability -v
PYTHONPATH=python python -m unittest eggserve.test_parity_matrix -v
```

### Packaging smoke tests

```sh
cd crates/eggserve-python/packaging-tests
bash run_all.sh ../dist/*.whl python3.14
```

## See Also

- [overview.md](overview.md) — Architecture overview
- [eggserve-core.md](eggserve-core.md) — Core library modules under test
- [eggserve-python.md](eggserve-python.md) — Python test suites
- [release-process.md](../docs/release-process.md) — Manual release procedure (Plan 091)
- [../docs/fuzzing.md](../docs/fuzzing.md) — Fuzzing documentation
