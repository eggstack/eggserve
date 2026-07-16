# Testing and Conformance — Deep Dive

eggserve uses a multi-layered testing strategy: Rust unit/integration tests, Python test suites, shared conformance corpora, fuzzing, and live HTTP wire tests.

## Test Layers

| Layer | Location | Scope | Count |
|-------|----------|-------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic | ~200+ |
| Rust integration tests | `crates/eggserve-core/tests/*.rs` | Cross-module, live TCP, TLS | 23 files |
| Rust bin tests | `crates/eggserve-bin/tests/*.rs` | Production binary paths | 1 file |
| Python native primitives | `python/eggserve/test_primitives.py` | PyO3 bindings, 143 tests | 143 |
| Python server primitives | `python/eggserve/test_server_primitives.py` | Server types, 68 tests | 68 |
| Python subprocess API | `python/eggserve/test_server.py` | CLI subprocess, 43 tests | 43 |
| Python server integration | `python/eggserve/test_server_integration.py` | Live concurrency/shutdown, 61 tests | 61 |
| Python canonical conformance | `python/eggserve/test_canonical_conformance.py` | Rust/Python parity, 92 tests | 92 |
| Python canonical request types | `python/eggserve/test_canonical_request_types.py` | Request type correctness, 61 tests | 61 |
| Python body primitives | `python/eggserve/test_body_primitives.py` | Body consumption, 52 tests | 52 |
| Python body conformance | `python/eggserve/test_body_conformance.py` | Body corpus parity, 11 tests | 11 |
| Python body wire | `python/eggserve/test_body_wire.py` | Wire-level body tests, 19 tests | 19 |
| Python boundary hardening | `python/eggserve/test_boundary_hardening.py` | Security hardening, 67 tests | 67 |
| Python client primitives | `python/eggserve/test_client_primitives.py` | HTTP client, 19 tests | 19 |
| Python API consumers | `python/eggserve/test_api_consumers.py` | API surface validation, 59 tests | 59 |
| Python API stability | `python/eggserve/test_api_stability.py` | Snapshot/import safety, 61 tests | 61 |
| Python parity matrix | `python/eggserve/test_parity_matrix.py` | Real-socket Rust/Python parity, 28 tests | 28 |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing | 19 targets |
| Conformance corpus | `conformance/*.json` | Shared Rust/Python test data | 2 corpora |

**Total: ~845+ Python tests, ~200+ Rust tests, 19 fuzz targets, 2 conformance corpora.**

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
PYTHONPATH=python python -m unittest eggserve.test_body_primitives -v
PYTHONPATH=python python -m unittest eggserve.test_boundary_hardening -v
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
- [release-infrastructure.md](release-infrastructure.md) — CI gate definitions
- [../docs/fuzzing.md](../docs/fuzzing.md) — Fuzzing documentation
