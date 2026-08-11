# Plan 115 — Verification Inventory by Invariant

Temporary working inventory. Deleted when Plan 115 closes.

---

## 1. Static Formatting/Lint

| Check | Location | Routine CI |
|-------|----------|------------|
| `cargo fmt --all -- --check` | `.github/workflows/ci.yml` (rust job) | Yes |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | `.github/workflows/ci.yml` (rust job) | Yes |
| `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings` | `.github/workflows/ci.yml` (rust job) | Yes |

---

## 2. Core Unit/Integration Correctness

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `eggserve-core/tests/api_stability.rs` | Public API shape stability (struct/enum field counts, method signatures) | Snapshot-style; unique |
| `eggserve-core/tests/canonical_conformance.rs` | Canonical type parsing (Method, StatusCode, HttpVersion, HeaderBlock, RequestTarget) against conformance corpus | Cross-language parity via `conformance/corpus.json` |
| `eggserve-core/tests/body_conformance.rs` | RequestBody state machine transitions, policy enforcement | Unique — state machine correctness |
| `eggserve-core/tests/body_primitives.rs` | Body construction, chunk iteration, consumption modes | Unique — construction API |
| `eggserve-core/tests/body_properties.rs` | Body metadata (declared length, kind, source) | Unique — property queries |
| `eggserve-core/tests/http_primitives_integration.rs` | Cross-module HTTP primitive interactions (RequestTarget + HeaderBlock + StatusCode) | Unique — integration glue |
| `eggserve-core/tests/lifecycle_integration.rs` | Server lifecycle state machine (Created → Starting → Running → Stopped) | Unique — state transitions |
| `eggserve-core/tests/server_integration.rs` | End-to-end serving (bind → request → response → shutdown) | Unique — full-stack |
| `eggserve-core/tests/ops_integration.rs` | Structured logging events, counters, field sanitization | Unique — observability |
| `eggserve-core/tests/no_hyper_in_public_api.rs` | Hyper types do not leak into public API surface | Unique — API hygiene |
| `eggserve-core/tests/public_api_consumers.rs` | Public API types are usable from external crate perspective | Unique — consumer ergonomics |
| `eggserve-core/tests/request_body_integration.rs` | RequestBody consumption across service boundary | Overlaps body_conformance but tests service-level integration |
| `eggserve-core/tests/unix_validator_qualification.rs` | Unix path validator exhaustive behavior | Unique — platform-specific (CI-gated: `#[cfg(unix)]`) |
| `eggserve-bin/tests/cli_validation.rs` | CLI argument parsing, error messages, help text | Unique — binary crate |
| `eggserve-bin/tests/logging_modes.rs` | `--log-format`, `--quiet` output modes | Unique — binary crate |
| `eggserve-bin/tests/production_path.rs` | Production profile flags produce expected runtime config | Unique — binary crate |

---

## 3. Raw-Wire HTTP Correctness

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `eggserve-core/tests/canonical_wire_interop.rs` | Canonical types serialize to valid HTTP wire format | Unique — wire encoding |
| `eggserve-core/tests/http_wire_correctness.rs` | Live TCP: method routing, TE+CL framing, asterisk-form, body policy, host validation | Unique — adversarial wire |
| `eggserve-core/tests/request_body_wire.rs` | Request body framing over live TCP (chunked, Content-Length, rejection) | Unique — body wire |

---

## 4. Filesystem Confinement/Security

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `eggserve-core/tests/qualification.rs` | Path confinement qualification (traversal, symlink, dotfile) | Unique — security qualification |
| `eggserve-core/tests/windows_plan084.rs` | Windows handle-relative child resolution | Unique — Windows (feature-gated: `windows-plan086`) |
| `eggserve-core/tests/windows_plan086.rs` | Windows handle-relative directory enumeration | Unique — Windows (feature-gated: `windows-plan086`) |
| `eggserve-core/tests/windows_feasibility.rs` | Windows feasibility scaffold | Unique — Windows (CI-gated: `#[cfg(windows)]`) |

---

## 5. Runtime Lifecycle/Resource Limits

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `eggserve-core/tests/streaming_buffer_qualification.rs` | Streaming buffer behavior, connection semaphore release, file-backed response bodies | Unique — streaming lifecycle; 2 tests `#[ignore]`d (semaphore architecture mismatch) |

---

## 6. Python API Compatibility

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `test_public_api.py` | `eggserve.__all__` exports are correct; removed surfaces absent | Unique — API surface |
| `test_primitives.py` | Canonical type construction from Python | Unique — type bridge |
| `test_canonical_request_types.py` | RequestTarget, RequestHead, HeaderBlock from Python | Unique — type bridge |
| `test_canonical_conformance.py` | Canonical type parsing parity with Rust | Cross-language parity |
| `test_http_server_compat.py` | `http.server` compatibility (handler, request, response) | Unique — compat facade |
| `test_https_server_compat.py` | TLS server compatibility from Python | Unique — TLS compat |
| `test_simple_http_handler_compat.py` | `SimpleHTTPRequestHandler` behavior parity | Unique — compat facade |
| `test_server.py` | Server lifecycle from Python (start, serve, stop) | Unique — Python server |
| `test_server_primitives.py` | Server builder options from Python | Unique — Python server |
| `test_server_integration.py` | End-to-end Python server with live requests | Unique — Python integration |
| `test_body_primitives.py` | RequestBody.read() / iter_chunks() from Python | Unique — Python body |
| `test_body_conformance.py` | Body state machine parity with Rust | Cross-language parity |
| `test_body_wire.py` | Body framing from Python over live TCP | Unique — Python wire |
| `test_boundary_hardening.py` | Edge cases: empty paths, null bytes, oversized headers | Unique — adversarial |
| `test_parity_matrix.py` | Rust/Python behavior parity via conformance corpus | Cross-language parity |

---

## 7. Python Installed-Wheel Behavior

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `test_imports.py` | Wheel imports succeed | Packaging smoke |
| `test_cli_smoke.py` | CLI binary runs from installed wheel | Packaging smoke |
| `test_server_smoke.py` | Server starts from installed wheel | Packaging smoke |
| `test_lifecycle_smoke.py` | Full lifecycle from installed wheel | Packaging smoke |
| `test_body_smoke.py` | Body handling from installed wheel | Packaging smoke |
| `test_client_smoke.py` | Client from installed wheel | **Stale** — references removed HttpClient |

---

## 8. TLS Behavior

| Test File | Invariant | Notes |
|-----------|-----------|-------|
| `eggserve-core/tests/tls_service_parity.rs` | TLS service behavior parity with plain HTTP | Unique — TLS (feature-gated: `tls`) |
| `eggserve-core/tests/request_body_tls.rs` | Request body handling over TLS connections | Unique — TLS body (feature-gated: `tls`) |
| `eggserve-bin/tests/tls_abuse.rs` | TLS error handling: invalid certs, handshake failures, protocol violations | Unique — TLS adversarial (feature-gated: `tls`) |

---

## 9. Packaging/Release Checks

| Tool | Location | Notes |
|------|----------|-------|
| `verify-cargo-packages.sh` | `scripts/` | Cargo package dry-run (core, bin) |
| `test-python-wheel.sh` | `scripts/` | Maturin build + install + smoke + tests |

---

## 10. Diagnostic/Stress/Fuzz/Interop Assets

### Fuzz Targets (11)

| Target | Invariant |
|--------|-----------|
| `request_target` | Request-target parsing, path confinement |
| `percent_decode` | Single-pass percent decoding |
| `path_components` | Path normalization, component validation |
| `validate_method` | HTTP method construction, body rejection |
| `range_header` | Range header parsing |
| `if_none_match` | If-None-Match ETag comparison |
| `platform_component` | Windows platform-specific checks |
| `fuzz_header_block` | HeaderName, HeaderValue, HeaderBlock operations |
| `fuzz_normalize_response` | StatusCode validation, response normalization |
| `fuzz_request_body` | RequestBody state machine |
| `fuzz_directory_buffer` | Directory listing buffer behavior |

### Stress/Diagnostic Tests

| Test | Invariant | Notes |
|------|-----------|-------|
| `eggserve-core/tests/fault_injection.rs` | Error isolation, permit release under fault | Manual/targeted |
| `eggserve-core/tests/filesystem_race_qualification.rs` | TOCTOU prevention under adversarial scheduling | Manual/targeted |
| `eggserve-core/tests/stateful_fuzz_replay.rs` | Stateful replay of fuzz-derived inputs | Manual/targeted |
| `eggserve-core/tests/request_body_cancellation.rs` | Body cancellation safety | Diagnostic |
| `eggserve-core/tests/request_body_timeout_interaction.rs` | Body timeout + limit interaction | Diagnostic |

### Conformance Corpora (2)

| Corpus | Consumers | Retention Reason |
|--------|-----------|-----------------|
| `conformance/corpus.json` | `canonical_conformance.rs`, `test_canonical_conformance.py` | Rust/Python canonical type parity |
| `conformance/body_corpus.json` | `body_conformance.rs`, `test_body_conformance.py` | Rust/Python body state parity |

### Proxy Interop (manual, optional)

| Script | Notes |
|--------|-------|
| `tests/proxy/caddy_interop.sh` | Caddy reverse-proxy interop |
| `tests/proxy/nginx_interop.sh` | nginx reverse-proxy interop |
| `tests/proxy/desync_corpus.sh` | Desync/smuggling corpus |

### Other

| Script | Notes |
|--------|-------|
| `tests/installed-binary-qual.sh` | Installed binary qualification |
| `tests/soak/soak_24h.sh` | 24-hour soak test |

---

## Identified Stale/Obsolete Items

1. **`test_client_smoke.py`** (packaging-tests) — references removed `HttpClient`. Should be deleted or rewritten.
2. **`scripts/verify.sh` line 62** — `--features client-tls` (feature doesn't exist). Dead command.
3. **`scripts/verify.sh` line 92** — `--features client` (feature doesn't exist). Dead command.
4. **`scripts/verify.sh` lines 8, 89, 129** — stale "TLS/client" comments.
5. **`architecture/error-taxonomy.md` line 3** — says "seven" (actual: six).
6. **`architecture/overview.md` line 65** — lists `ClientError` as 7th layer.
7. **`architecture/overview.md` line 95** — says "12 fuzz targets" (actual: 11).
8. **`architecture/overview.md` line 172** — says "12 fuzz targets" (actual: 11).
9. **`architecture/primitives-api.md` lines 373-378** — `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientResponse`, `ClientError`, `Scheme`/`ParsedUrl` entries (removed subsystem).
10. **`architecture/testing-and-conformance.md` line 113** — `url_parse` fuzz target row (target removed).
11. **`.opencode/skills/eggserve-dev/SKILL.md` line 72** — "Seven distinct error types" + lists `Client` variant and `ClientError`.
12. **`docs/extension-contract.md` lines 165-170** — `HttpClient`, `ClientConfig`, `ClientRequest`, `ClientRequestBuilder`, `ClientResponse`, `ClientError` entries (removed subsystem).
