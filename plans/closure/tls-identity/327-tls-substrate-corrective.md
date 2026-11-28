# TLS Identity Milestone 327 - Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tls-identity/327-tls-substrate-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7`

Repository baseline reviewed: `26649bd` (working tree; changes not committed by this agent)

Implementation commits or pull requests:

- not yet committed (working tree on `26649bd`; commit is a user-owned action)

## 1. Executive finding

The five reviewed TLS seam defects are corrected and each has a regression test that fails on the pre-fix behavior: provider-explicit construction, ALPN/HTTP2 coherence validation, the documented header/TLS timeout coupling, the order-independent SNI identity ceiling, and typed H3 QUIC transport-bound errors. Every milestone-owned lane passes, including the Python crate check and clippy, the 1.89 MSRV check, and the H3 feature lanes.

The broad wrapper `./scripts/verify.sh fast` did not complete on this macOS host at the original baseline because of two pre-existing failures in untouched crates (details in section 10); the equivalent remaining lanes were run directly and are recorded below. After the merge with main, Plan 323's canonicalization fix cleared the `eggserve-static` one; only the platform-specific `eggserve-server` test still fails locally.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Construction never depends on a process-default rustls provider | `eggnet-tls` unit test `construction_does_not_require_a_process_default_provider`; test graph compiles both `ring` and `aws-lc-rs` | pass | Pre-fix this test panics in `ServerConfig::builder()`. |
| TLS ALPN agrees with HTTP/2 availability | `tls_identity::tls_alpn_h2_requires_runtime_http2` (static config and reload snapshot) | pass | Pre-fix `validate()` returned `Ok`; the accept loop dropped the connection after the handshake. |
| `--header-timeout` bounds TLS handshakes | `args::tests::header_timeout_also_bounds_tls_handshakes`; `docs/tls.md`, `docs/security-policy.md`, `docs/deployment.md`, `docs/cli.md`, `docs/timeout-reference.md` updated | pass | Pre-fix only `header_read_timeout` moved. |
| Named-identity ceiling is order-independent | `neutral_tls::identity_ceiling_excludes_the_optional_default` | pass | Default-first now accepts 64 named identities; the 65th named identity is rejected. |
| H3 transport bounds are validated and typed | `Http3Config` unit tests for `stream_receive_window`, `connection_receive_window`, `max_idle_timeout`; `quic.rs` uses `InvalidTransportConfig` | pass | Pre-fix out-of-range values surfaced as `invalid private key`. |

## 3. Production implementation evidence

- `crates/eggnet-tls/src/lib.rs`: one `crypto_provider()` helper (ring) now serves `build_single_cert_config`, `TlsServerConfigBuilder::build`, `WebPkiClientVerifier::builder_with_provider`, and key-pair validation. Added `TlsError::{InvalidCryptoProvider, InvalidTransportConfig}`. `add_identity` no longer counts the optional default against `MAX_TLS_IDENTITIES`.
- `crates/eggserve-h3/src/quic.rs`: explicit ring provider plus TLS 1.3 protocol versions; VarInt/IdleTimeout range failures use `InvalidTransportConfig`.
- `crates/eggserve-h3/src/config.rs`: `Http3Config::validate` enforces the QUIC VarInt ceiling for both receive windows and the idle timeout, naming the field.
- `crates/eggserve-core/src/server/config/runtime.rs`: `RuntimeConfig::validate` checks the effective TLS configuration (static or current reload snapshot) for an `h2` advertisement when HTTP/2 is disabled or the build lacks the feature.
- `crates/eggserve-bin/src/args.rs`: `--header-timeout` sets both `header_read_timeout` and `tls_handshake_timeout`; usage text updated. Two Plan 320 bind-parsing tests gained the now-required `--public` acknowledgment (out-of-scope incidental; see section 10).
- `crates/eggserve-python/src/server/runtime.rs`: removed the process-global `install_default()` workaround now that every construction path is provider-explicit.
- Documentation: `docs/tls.md`, `docs/security-policy.md`, `docs/deployment.md`, `docs/cli.md`, `docs/timeout-reference.md`, `architecture/eggnet-tls.md`, `CHANGELOG.md`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
git diff --check
python3 scripts/check-crate-topology.py
cargo test -p eggnet-tls
cargo test -p eggserve-h3
cargo test -p eggserve-core --features tls --test tls_identity
cargo test -p eggserve-core --features http2,tls --test tls_identity
cargo test -p eggserve-core --features http2,tls
cargo test -p eggserve-core --features http3,tls
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-bin --features http2,tls
cargo test -p eggserve-bin --features http3,tls
cargo clippy -p eggnet-tls --all-targets -- -D warnings
cargo clippy -p eggserve-h3 --all-targets -- -D warnings
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings
cargo +1.89 check -p eggserve-core --all-targets --features http2,tls
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
cargo clippy --manifest-path fuzz/Cargo.toml --bins -- -D warnings
./scripts/verify.sh fast   # stopped on a pre-existing unrelated test
cargo test --workspace     # stopped on a pre-existing macOS-only test
```

### Results

- `eggnet-tls`: 3 unit + 9 integration tests passed, including the provider and identity-ceiling regressions.
- `eggserve-h3`: 15 unit tests passed, including both QUIC bound tests.
- Core TLS identity: 16 tests passed without `http2`; 18 passed with `http2,tls`, including `tls_alpn_h2_requires_runtime_http2`.
- Core suites with `http2,tls` and `http3,tls`: exit 0.
- CLI suites: `--features tls` ran 145 tests across six targets with 0 failures (includes the 12-test TLS abuse lane); `http2,tls` and `http3,tls` lanes exited 0.
- Clippy: every listed lane exited 0, including workspace-wide and the excluded Python crate.
- Python crate `cargo check --locked` and clippy: exit 0.
- MSRV `cargo +1.89 check`: exit 0.
- Topology gate, formatting, `git diff --check`, fuzz check/clippy: pass.
- `./scripts/verify.sh fast`: exit 101 at `eggserve-server` test `app_bounded_body_accepts_and_over_limit_rejects` (ConnectionReset, macOS); never reached later lanes, which were run manually.
- `cargo test --workspace`: exit 101 at the pre-existing `eggserve-static` test `fs::tests::resolve_child_directory_retains_canonical_path` (macOS `/private/var` canonicalization).

## 5. Invariant review

- TLS defaults are unchanged: no 0-RTT, TLS 1.3-only QUIC, rustls default ticket policy.
- The ring provider was already the key-validation provider; handshakes now use the same provider instead of a process default.
- CLI and Python remain single-identity; Python remains HTTP/1.1-only; no new capability tier is claimed.
- No key/certificate bytes are added to errors, Debug output, events, or logs; the new errors name configuration fields.
- Bounded metadata (SNI 253, chain 8 x 64 KiB), trust/CRL bounds, and safe defaults are untouched.

## 6. Failure and recovery review

- Static configuration contradictions fail at `RuntimeConfig::validate` (called by `ServerBuilder::build` and `Server::start`).
- A reload that introduces an `h2` advertisement on an HTTP/2-disabled runtime is rejected by the per-connection `validate()` call, so connections fail closed; established sessions are untouched. **Correction (milestone 328):** an h2-negotiating client does not reach that validator, because `accept_tls` drops the connection first; the swap-time guard in milestone 328 is the enforcement point.
- Provider selection is stateless and deterministic; no new lock or shared state.
- The identity-cap change does not alter handshake or reload behavior.

## 7. Migration and compatibility review

- `TlsError` gains two variants (`InvalidCryptoProvider`, `InvalidTransportConfig`). This is a 0.x diagnostic adjustment; no crate version is bumped in this milestone.
- `--header-timeout` now also bounds TLS handshakes, matching the already-published security policy. An operator lowering the flag tightens both phases; raising it relaxes both.
- Removing the Python process-global provider install has no observable TLS behavior change because construction is explicit.
- No configuration field, default, or public builder method is added or removed.

## 8. Security review

- Removes a process-abort path on valid credentials in multi-provider dependency graphs.
- Rejects protocol advertisement the runtime cannot serve, instead of a post-handshake drop.
- Keeps the documented 64 named-identity resource ceiling order-independent.
- The QUIC transport error no longer masquerades as a private-key failure.

## 9. Documentation and operations

Updated `docs/tls.md`, `docs/security-policy.md`, `docs/deployment.md`, `docs/cli.md`, `docs/timeout-reference.md`, `architecture/eggnet-tls.md`, and `CHANGELOG.md`. The timeout reference now records the PROXY-preamble ordering and the CLI coupling.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `eggserve-server` test `app_bounded_body_accepts_and_over_limit_rejects` fails on this macOS host with `ConnectionReset` at the client `read_to_end` (line 205) | `./scripts/verify.sh fast` cannot complete locally | Fix separately in `direct-h1-runtime`: tolerate the reset or assert the 413 on a platform-stable read path. Untouched by this milestone and reproduces alone. |
| low | `eggserve-static` test `resolve_child_directory_retains_canonical_path` fails because macOS resolves `/var` to `/private/var` | `cargo test --workspace` could not be green locally at the original baseline | **Resolved upstream:** main's Plan 323 fixed the canonicalization comparison before this branch merged; `cargo test --workspace` is now green locally. |
| low | Two Plan 320 bind-parsing tests (`bind_and_port_separate_flags`, `bind_host_only_preserves_default_port`) lacked the `--public` acknowledgment required by the baseline gate | The CLI lib suite failed before this milestone's changes | Corrected here by adding `--public`; test intent is preserved. |

## 11. Roadmap disposition

Milestone 327 is closed; no hard dependency blocks the next TLS identity or H2/H3 milestone. The remaining platform-test finding is owned by the `direct-h1-runtime` roadmap.

## 12. Registry updates

- `plans/registry.md`: TLS identity milestone 327 marked closed with this record; next milestone number 329.
- `plans/subsystems/tls-identity-roadmap.md`: milestone 327 marked closed.
- `plans/implementation/tls-identity/327-tls-substrate-corrective.md`: status implemented.

## 13. Post-closure correction (review follow-up)

- `crates/eggserve-h3/src/config.rs`: the send-buffer check combined `send_window`
  and `max_send_buf_size` and always named `http3.send_window`, so a zero
  `max_send_buf_size` produced a false field attribution. Split into two arms
  (matching `http2.rs`), with `zero_send_buf_size_names_its_own_field` covering
  both branches.
- Low finding: `max_send_buf_size` was bounded only below (`> 0`), and
  `send_window` had no upper bound even though quinn documents it as the
  per-connection retained-write memory cap. The H3 knobs are Rust-API only (no
  CLI or Python surface), and the defaults stay at 4 MiB / 256 KiB.
- **Resolved by milestone 329**
  (`plans/closure/tls-identity/329-h3-send-window-bound.md`): `send_window` and
  `max_send_buf_size` are bounded to `1..=u32::MAX`, matching
  `http2.max_send_buf_size`, with field-named diagnostics and unit tests at and
  above the ceiling. The QUIC VarInt ceiling (`2^62-1`) remains on the
  wire-encoded fields only.
