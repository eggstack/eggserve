# Plan 192 HTTP/3 Dependency Readiness

Date: 2026-09-10
Candidate base: `7643afa6afedd6bdae52fd0efeffc97f026f632d`
Environment: Linux x86_64, loopback qualification
Decision: **BLOCKED — H3 remains experimental; do not execute Plan 193 until the blockers below are resolved through a later narrow readiness update.**

## Scope

Plan 192 is a dependency-readiness and conformance-hardening gate, not a
promotion. No transport capability was added. The pass re-evaluated the
`h3` / `h3-quinn` / Quinn stack at its latest released versions, triaged the
named upstream issues against EggServe's exact server-side code paths, audited
QPACK/control/resource ownership, stream termination, flow control,
connection lifetime, Alt-Svc, and TLS configuration, applied only narrow
source hardening with regression tests, and hardened the qualification script
so Plan 193 evidence classes fail closed.

## Candidate stack (Track A)

Inventory taken 2026-09-10 from `Cargo.lock` and the crates.io registry.
There is no newer maintained release to move to: `h3` 0.0.8 (2025-05-06) and
`h3-quinn` 0.0.10 (2025-05-06) remain the latest published versions, and the
Quinn 0.11 line is at its latest patch.

| Crate | Version in lockfile | Latest released | Disposition |
|---|---|---|---|
| `h3` | 0.0.8 | 0.0.8 | current stack accepted (only release) |
| `h3-quinn` | 0.0.10 | 0.0.10 | current stack accepted (only release) |
| `quinn` | 0.11.11 | 0.0.11.11 (2026-06-22) | current, latest patch |
| `quinn-proto` | 0.11.17 | 0.11.x line | current via `quinn` requirement |
| `quinn-udp` | 0.5.15 | 0.5.x line | current via `quinn` requirement |
| `rustls` | 0.23.41 | 0.23.x line | current, no advisory |
| `rustls-pki-types` | 1.15.0 | 1.x line | current |
| `ring` | 0.17.14 | 0.17.x line | current QUIC crypto backend |

- Features: `h3`/`h3-quinn` with `default-features = false`; `quinn` with
  `runtime-tokio` + `rustls-ring` only. No WebTransport, datagram, or
  extended-CONNECT features are enabled.
- MSRV: `h3-quinn` declares 1.70, Quinn declares 1.74.1; both are below the
  workspace floor of Rust 1.88, which is preserved (`cargo +1.88 check`
  gates pass).
- Supply chain: `cargo audit` reports no vulnerabilities for the locked
  graph; `cargo deny check` passes (advisories, bans, licenses, sources).
- Footprint: H3/QUIC crates remain behind the `http3` feature (which implies
  `tls`). The no-feature tree contains no `h3`, `h3-quinn`, or `quinn`
  (enforced by `scripts/qualify-http3.sh`); the default build is unchanged.

Result: **CURRENT STACK ACCEPTED** as the frozen candidate — with the
blockers below, not as a promotion input.

## Upstream issue inventory and dispositions

Checked 2026-09-10 against `hyperium/h3` issues/PRs and the Quinn release
history.

### hyperium/h3#338 — buffered H3 data lost on same-batch connection error (OPEN, BLOCKER)

- Status: issue open since 2026-04-27; fix PR hyperium/h3#339 open and
  unmerged. No released `h3` version contains the fix.
- Applicability: applicable. The report targets the same `h3 0.0.8` /
  `h3-quinn 0.0.10` family, and EggServe's server-side path traverses the
  affected frame layer: request resolution (`resolver.resolve_request()` →
  `FrameStream::poll_next` for HEADERS) and request DATA
  (`recv_stream.recv_data()` → `poll_recv_data` / `poll_data`). A request or
  body arriving in the same UDP batch as a connection-level error can be
  misclassified as `StreamError::ConnectionError` instead of the valid
  application data already buffered.
- Mitigation: none applied. A correct fix belongs at the h3 frame layer
  (drain buffered bytes before propagating the cached QUIC error, as #339
  proposes). Vendoring or forking `h3` to force a supported label is
  explicitly out of scope for this plan.
- Regression: `h3_complete_response_survives_immediate_peer_close` in
  `crates/eggserve-core/tests/http3_runtime.rs` covers the closest
  deterministic simulation available in-process (a complete response observed
  across an immediate peer close, plus a fresh connection proving the server
  endpoint stays usable). Exact kernel-batched coalescing cannot be
  reproduced deterministically here; it stays a Plan 193 platform run.
- Disposition: **blocks promotion** until a maintained upstream release
  carries the #339-class fix (or a successor) through normal
  compatibility/MSRV/security review.

### hyperium/h3#262 — unfinished RequestStream drop/reset behavior (OPEN, BLOCKER)

- Status: still open. Dropping an H3 request stream does not automatically
  perform the RFC 9114 §4.1.1-recommended reset/abort of unfinished
  directions; the `RequestEnd` drop only notifies the connection of request
  end.
- Applicability: applicable. After `RequestStream::split`, each half must be
  stopped explicitly (`stop_stream` for send, `stop_sending` for receive).
  Audit found three pre-body paths that dropped the receive half without an
  explicit abort: request-head conversion errors, declared-length errors, and
  the Reject presence-probe receive-error branch (which stopped send only).
  This plan fixes those three narrowly (explicit
  `H3_REQUEST_CANCELLED` on receive; see source changes below).
- Residual paths that still drop receive without an explicit public-API
  abort and therefore also block promotion:
  - 503 service-admission rejection when a Stream-policy body already owns
    the receive half (moved into `RequestBody` before the permit check);
  - Buffer-policy `read_all` failures (the unfold stream owning receive is
    consumed by `read_all(self)` and dropped on error);
  - post-service unconsumed Stream bodies (request dropped after
    response-start with an Active body; lifecycle cancellation is
    stream-scoped but no `STOP_SENDING` is issued on the QUIC stream).
  - These are bounded at runtime by the body watchdog, `response_write_timeout`,
    and connection lifecycle, but they do not meet the acceptance bar of "no
    ordinary stream-level failure leaks an unfinished stream until idle
    timeout solely because a task/future was dropped."
- Regression: `h3_early_head_error_is_stream_scoped_and_sibling_survives`
  proves an early 400 terminates only its own stream while a sibling
  completes, and the existing presence-probe, probe-timeout, peer-close, and
  shutdown tests continue to pass.
- Disposition: **blocks promotion** until every termination path has explicit
  send/receive ownership through public APIs (or upstream closes #262 with a
  reliable drop semantic EggServe can depend on).

### Other upstream issues (reviewed, no new blocker)

- hyperium/h3#308 (unbounded `BufRecvStream` buffering) with PR #328
  (buffering rework, WIP/unmerged): EggServe's exposure is bounded in
  practice — request bodies are pull-driven through the canonical
  `RequestBody` with declared-length and `max_request_body_bytes` ceilings,
  and the Reject path performs at most one bounded probe read — but the
  underlying frame-layer buffering is dependency-owned and unmerged work.
  Noted as Plan 193 adversarial evidence, not a separate blocker beyond the
  two named issues.
- Quinn 0.11.x release history shows only routine patches since the pinned
  0.11.11; no QUIC transport, retry, migration, or 0-RTT behavior change
  affecting EggServe's configured subset was found. Migration stays disabled,
  0-RTT stays refused (`max_early_data_size = 0`).
- WebTransport/datagram/extended-CONNECT issues were excluded per plan scope
  (features not enabled); none revealed a shared-machinery defect in the
  ordinary request path.

## Standards and ownership matrix (dated 2026-09-10)

Baseline: RFC 9110 (semantics), RFC 9114 (HTTP/3), RFC 9000 (QUIC
transport), RFC 9001 (QUIC TLS), RFC 9002 (loss/congestion), RFC 9204
(QPACK), RFC 7838 (Alt-Svc), RFC 7301 (ALPN), TLS 1.3 (RFC 8446 as
superseded; plan baseline names RFC 9846).

| Requirement | Owner | EggServe evidence |
|---|---|---|
| HTTP semantics, method/target/authority/headers, body policy, status/reason/body | EggServe (canonical adapter + shared kernel) | deterministic H3 tests; canonical error constructor shared with H1/H2 |
| Service admission, panic containment, handler/body timeouts, normalization, privacy | EggServe (runtime) | shared `RuntimeState`, `invoke_canonical_service`, `finalize_canonical_response` |
| Alt-Svc generation/suppression, same-port advertisement | EggServe | same-port + denylist tests; script checks `h3=":<port>"` |
| QUIC stream/connection windows, idle timeout, pending handshakes, field-section limit, send buffer, stateless retry | EggServe-configured (`Http3Config` → Quinn/h3 builder) | validation tests; `load_quic_server_config` projection |
| Dual-listener lifecycle, GOAWAY drain, graceful shutdown | EggServe (supervisor) | shutdown/drain/max-requests tests |
| QUIC packet recovery/congestion, TLS key schedule, retry/amplification mechanics | Dependency (Quinn/rustls/OS) | version pin + audit; representative evidence deferred to Plan 193 |
| QPACK codec correctness, H3 frame parser correctness | Dependency (h3) | version pin; #338 blocker recorded above |
| H3 control-stream uniqueness, unknown stream handling, malformed SETTINGS/frame placement | Dependency (h3 connection driver) | pass-through; adversarial harness deferred to Plan 193 (`h3i` gate added to script) |

## QPACK and resource ownership (Track B)

`Http3Config` explicitly bounds: bidi streams (100), uni streams (16, ≥3
headroom validated for control/QPACK), per-stream receive window
(256 KiB), connection receive window (4 MiB, ≥ stream window validated),
send window (4 MiB), idle timeout (60 s), pending handshakes (64),
field-section size (32 KiB, ≥1024 validated), per-response send buffer
(256 KiB), stateless retry (explicit bool, default off), Alt-Svc (explicit
bool, default off).

| Additional state | Disposition |
|---|---|
| QPACK dynamic table / blocked streams / encoder-decoder stream state | Bounded by dependency default: EggServe's request path uses stateless decode (`decode_stateless` capped by `max_field_section_size`) and responses use stateless encode; no dynamic-table capacity knob is exposed because the used subset does not grow dynamic state. Pinned via `h3` version. |
| Control-stream uniqueness / duplicate QPACK streams | Dependency-enforced connection errors in the h3 driver; no EggServe knob (category 3). Adversarial proof deferred to Plan 193. |
| Retained reset/error state per stream | Drained through explicit EggServe stop actions where owned (see termination table); residual paths listed as blockers above. |
| Maximum encoded/decompressed field behavior | Explicit: `max_field_section_size` projected to the h3 builder on both accept and field decode. |
| Receive-memory ceiling | Documented envelope: `max_concurrent_bidi_streams × stream_receive_window` capped by the connection window per connection, plus bounded uni/control state; connection count additionally capped by the shared `max_connections` semaphore. |

No new public configuration knob was added: every additional resource is
either explicitly configured, a documented bounded dependency default, or a
recorded blocker. No relevant unbounded state remains unexamined.

## Stream termination and cancellation (Track C)

Conventions: `H3_REQUEST_CANCELLED` aborts receive of a rejected/unprocessed
request; `H3_INTERNAL_ERROR` resets send after response commitment;
`H3_REQUEST_INCOMPLETE` is h3-internal for headerless termination.
`RequestLifecycle` reasons reuse the transport-neutral taxonomy
(PeerDisconnected / ServerShutdown / ConnectionTimeout / TransportFailure,
first wins). Sibling streams stay alive except on connection-wide events.

| Path | Response before commitment | Send action | Receive action | Lifecycle | Sibling |
|---|---|---|---|---|---|
| Normal completion | yes (service) | `finish()` | end-of-stream | Complete | alive |
| Head conversion error | yes (canonical 4xx) | error response + `finish()` | **`stop_sending(H3_REQUEST_CANCELLED)` (new)** | n/a (pre-registration) | alive |
| Declared-length error | yes (canonical 400) | error response + `finish()` | **`stop_sending(H3_REQUEST_CANCELLED)` (new)** | n/a | alive |
| Declared over body limit | yes (413) | error response | `stop_sending(H3_REQUEST_CANCELLED)` | n/a | alive |
| Reject + declared > 0 | yes (413) | error response | `stop_sending(H3_REQUEST_CANCELLED)` | n/a | alive |
| Reject probe finds DATA | yes (413) | error response | `stop_sending(H3_REQUEST_CANCELLED)` | Complete(empty body) | alive |
| Reject probe clean EOF | yes (service, empty body) | service response | end-of-stream | Complete | alive |
| Reject probe receive error | yes (500) | error response + `stop_stream(H3_INTERNAL_ERROR)` | **`stop_sending(H3_REQUEST_CANCELLED)` (new)** | Failed | alive |
| Reject probe timeout | yes (408) | error response | `stop_sending(H3_REQUEST_CANCELLED)` | Failed(ConnectionTimeout) | alive |
| Body read/size failure (Buffer) | yes (4xx/500) | error response | residual: dropped without explicit stop (BLOCKER) | Failed | alive |
| Service 503 admission (Stream body owns receive) | yes (503) | error response | residual: dropped without explicit stop (BLOCKER) | n/a | alive |
| Post-service unconsumed Stream body | already committed | `stop_stream(H3_INTERNAL_ERROR)` on send failure | residual: no explicit `STOP_SENDING` (BLOCKER) | Active→Abandoned/Failed via watchdog | alive |
| Service panic/timeout, handler timeout | yes (canonical 500/504) | error response or `stop_stream` | owned where receive still held; residual otherwise | Failed/Timeout | alive |
| Response producer error / length mismatch / send timeout | post-commit | `stop_stream(H3_INTERNAL_ERROR)` | n/a (receive already terminal) | TransportFailure on send failure | alive |
| Peer RESET/STOP/close | no | task-local failure handling | stream-local cancel | PeerDisconnected | alive |
| H3 connection error | no | registry cancel (connection-wide, correct: transport unusable) | same | mapped close reason | connection gone |
| Graceful shutdown / drain deadline / task abort / max-requests drain | in-flight complete, then GOAWAY | `shutdown(0)` then abort at deadline | `ServerShutdown` cancel | ServerShutdown | drained |

Acceptance is **not** met: the three residual rows above can leave an
unfinished QUIC stream without an explicit reset until idle timeout. That is
the #262 remainder and part of the BLOCKED decision.

## Flow control and no-progress (Track F)

- Response sends (`send_response`, each bounded `send_data` chunk, `finish`)
  are each wrapped in `response_write_timeout` (default 30 s). Chunking at
  `min(max_send_buf_size, stream_chunk_size)` re-arms the deadline per chunk,
  so a slow but progressing peer does not spuriously time out, while a peer
  withholding receive credit stalls the send future and the timeout fires per
  stream. A send failure after commitment resets only the affected stream
  (`H3_INTERNAL_ERROR`); siblings stay usable (proven by the sibling tests).
- Response buffers stay bounded by the send-chunk ceiling; nothing is
  buffered to avoid the timeout.
- Application producer stalls: a `ResponseStream` that never yields keeps its
  response task parked, but it is bounded by `response_write_timeout`
  (no-progress on the send path once a chunk is due), the QUIC idle timeout,
  graceful-shutdown drain, and the (documented, unenforced-for-H3-lifetime)
  connection ceiling interaction below. No new producer deadline was added:
  existing handler/stream semantics plus the documented bounds already cover
  it, and the residual risk is the stream-local reset remainder above, not an
  unbounded pin.
- Independent flow-control verification (real-credit withholding with two
  clients) remains Plan 193 evidence; the deterministic tests prove
  stream-scoping and timeout representation (408 via the canonical
  constructor), not wire-level credit behavior.

## Connection lifetime and idle semantics (Track G)

Decision: **document QUIC idle plus per-operation/request deadlines as the
H3 contract; `connection_total_timeout` does not bound QUIC/H3 connection
lifetime.** The H3 adapter enforces `max_idle_timeout` (QUIC, default 60 s),
`tls_handshake_timeout`, `body_read_timeout`, `handler_timeout`,
`response_write_timeout`, `max_requests_per_connection` drain, and the
graceful-shutdown deadline, but no total-lifetime timer exists on the QUIC
path. This is now stated in `docs/timeout-reference.md` and
`architecture/http3.md` so no field reads as protocol-neutral while H3
ignores it. Builder validation still requires handler/body budgets to fit
under an explicit `connection_total_timeout`, which constrains H3 handler/body
values without imposing an H3 lifetime timer. A future plan may impose a
QUIC-lifetime ceiling narrowly; this plan makes no such behavior change.

## Alt-Svc and endpoint correctness (Track H)

Re-verified without behavior change: port `0` resolves one origin port shared
by TCP and UDP (UDP bind failure returns before the server task starts and
drops the TCP listener); application responses cannot override runtime-owned
`Alt-Svc` (connection/header fields are stripped at finalization and
`Alt-Svc` is generated from the resolved listener); the response-policy
`alt-svc` denylist suppresses advertisement; H3-disabled or identity-less
states advertise nothing; TCP/H1/H2 stays healthy when H3 is disabled (script
fallback check). No DNS HTTPS/SVCB work was undertaken.

## TLS/QUIC security (Track I)

Unchanged and re-verified: QUIC uses a fresh rustls `ServerConfig` pinned to
TLS 1.3 only, ALPN exactly `h3`, `max_early_data_size = 0` (no application
0-RTT), no client auth, no weak/legacy modes; certificate/key handling reuses
the existing PEM loader ownership (`ServerBuilder::http3_identity` supplies
paths; key material never enters the public API); no transport secrets,
connection IDs, tokens, or packets are logged (adapter logs carry no QUIC
identifiers or raw values); stateless retry remains explicit policy
(`Http3Config::stateless_retry`, default off). No ECH, PQ, client-auth, or
automation work was added.

## Qualification tooling (Track K)

`scripts/qualify-http3.sh` now reports a Plan 193 evidence-class summary on
every run (direct clients, adversarial client, browser evidence, impairment
evidence, platform) where unavailable reads as `SKIP`, never `PASS`, and
fail-closed gates were added: `EGGSERVE_REQUIRE_ADVERSARIAL_H3`,
`EGGSERVE_REQUIRE_H3_BROWSER`, `EGGSERVE_REQUIRE_H3_IMPAIRMENT`,
`EGGSERVE_REQUIRE_H3_PLATFORM` (each exits 2 when the evidence is missing).
Existing `EGGSERVE_REQUIRE_H3_CLIENTS` / `EGGSERVE_REQUIRE_TWO_H3_CLIENTS`
behavior is unchanged. On this host (no H3 client, no h3i, Linux) the
baseline passes and every strict gate exits 2 as designed.

## Narrow source changes and tests

- `crates/eggserve-core/src/server/http3.rs`: explicit receive-direction
  abort (`H3_REQUEST_CANCELLED`) on the three pre-body early-error paths
  that previously relied on drop (head-conversion error, declared-length
  error, Reject probe receive error). No dependency, API, config, or
  behavior change beyond prompt stream termination.
- `crates/eggserve-core/tests/http3_runtime.rs`: two new deterministic
  tests — `h3_early_head_error_is_stream_scoped_and_sibling_survives`
  (Track C/#262) and `h3_complete_response_survives_immediate_peer_close`
  (Track D/#338 simulation). Full H3 suite: 9/9 pass.
- `scripts/qualify-http3.sh`: evidence-class summary + four fail-closed
  Plan 193 gates (test-only tooling, no runtime graph impact).

## Verification

On the execution tree: `cargo fmt --check`, `cargo +1.88 check`
(default and `--features http3,tls`), `cargo clippy` with `-D warnings`
(workspace default, `http3,tls` for both crates), `cargo test --workspace`,
`cargo test -p eggserve-core --features http3,tls` (including the 9-test H3
suite), `cargo test -p eggserve-bin --features http3,tls`, `cargo audit`,
`cargo deny check`, and the `qualify-http3.sh` baseline plus strict-gate
probes all pass. Routine H1/H2/Python behavior is unaffected (no shared-path
change; H3 edits are confined to the `http3` feature adapter and its tests).

## Final result

**`BLOCKED`.** The frozen candidate is `h3 0.0.8` / `h3-quinn 0.0.10` /
`quinn 0.11.11` / `rustls 0.23.41` (latest released; no upgrade available),
with the three narrow stream-termination fixes and two regression tests in
this pass. Promotion is blocked by:

1. **hyperium/h3#338 (open, no released fix)** — server-side request/DATA
   paths traverse the affected frame layer; cannot promote until a
   maintained release carries the drain-before-error fix.
2. **hyperium/h3#262 remainder (open)** — three early paths now fixed, but
   the 503-admission, Buffer-error, and post-service unconsumed-body paths
   still drop receive without an explicit public-API abort; cannot promote
   until every path owns both directions.
3. **Plan 193 evidence still missing by design** — independent clients,
   adversarial frames, impairment, browser, and non-Linux runtime evidence
   remain uncollected (the harness now represents them and fails closed).

H3 remains **experimental**. Plan 193 must not execute until a later narrow
readiness update resolves blockers 1–2 and re-freezes the candidate; blocker
3 is Plan 193's own campaign. Reaching BLOCKED with concrete, auditable
reasons is the successful outcome of this gate.
