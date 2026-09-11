# Plan 208 — HTTP Server Foundation Release Closure (closure record)

**Status:** Closed 2026-09-11 on `main`. Terminal closure for the Plan 196
program. Final commit: the `Close Plan 208 HTTP server foundation release
closure` commit on `main` (resolve with
`git log --oneline --grep='Plan 208'`).

## Program inventory (Plans 197–207)

| Plan | Disposition | Evidence |
|---|---|---|
| 197 application service contract | Implemented | `application_service_contract.rs`, `application_service.rs` example, normative contract in `docs/downstream-app-server.md` |
| 198 trailers + interim | Implemented, experimental | `trailers_interim` suite, `Trailers`/`InterimSender` types |
| 199 generic tunnel / Extended CONNECT | Implemented, experimental; supersedes deferred Plan 176 | `tunnel_upgrade.rs` (+ `tokio-tungstenite` fixture), one-shot `TunnelCapability` + `TunnelIo` |
| 200 `http`/`http-body`/Tower interop | Implemented, experimental, optional | `interop_http_tower.rs`, `docs/http-interop.md`; never in default builds |
| 201 listener / process-manager integration | Implemented, experimental | prebound/systemd/Unix/H3-socket paths, `ServerHandle::endpoints()` |
| 202 trusted proxy / PROXY protocol | Implemented, experimental | `trusted_proxy` suite, fail-closed provenance rules |
| 203 production TLS identity / mTLS / reload | Implemented, experimental, Rust-first | `tls_identity.rs`, `docs/tls.md`; CLI/Python stay single-identity |
| 204 async Python substrate | Implemented, experimental, H1-only | `test_async_bridge.py`, `asgi_fixture.py` (fixture only, not a product) |
| 205 observability hooks | **Explicitly deferred** | No `RequestObserver`/request-ID/lifecycle-event/timing API added. The Plan 181 per-runtime `OpsContext` (sink + counters + connection IDs + snapshots) remains the observability boundary. Rationale: no concrete downstream consumer requires the extension yet; adding a second event/observer system without a consumer would freeze a premature abstraction against the no-broad-surface rule. Reopen with a concrete downstream tracing/metrics integration need. |
| 206 module boundaries | Implemented, behavior-preserving | Re-exports preserve public paths; `pub(super)` discipline |
| 207 cross-protocol conformance | Implemented | `conformance/app_server_conformance.toml` (55 scenarios, 47 routine) + `cross_protocol_conformance.rs` routine subset; see `release/plan-207-cross-protocol-conformance.md` |

## Support-tier decision (Tracks B + C)

No promotion follows from this closure. Stability is a promise about
API/semantic maturity, not a reward for finishing the plan.

| Area | Tier | Notes |
|---|---|---|
| HTTP/1.1 static serving + Python `http.server` facade | **Supported** | Loopback/safe defaults, regression suite; Python stays H1.1-shaped |
| Canonical `primitives` value/plan types | **Supported (semver-considered pre-1.0)** | Method/authority/headers/target/status/response/body planning; the two Hyper adapters stay the only Hyper mentions in public signatures (outbound body opaque) |
| `eggserve-core::server` runtime (`Server`, `Service`, drivers, `RuntimeState`) | Experimental | Qualified by Plans 175/197/207 but API may still change before 1.0 |
| Trailers / interim 1xx | Experimental | Upstream emission-scoping limits documented in `http-primitives.md` |
| Generic tunnel / Extended CONNECT | Experimental | H3 generic `:protocol` blocked by `h3` 0.0.8 |
| `http-interop` / `tower` adapters | Experimental, optional | Loss-aware; never default |
| Listener injection / systemd / Unix / prebound H3 UDP | Experimental | Platform evidence incomplete |
| Trusted proxy / PROXY / forwarded-effective | Experimental | H3 ignores by design; still no reverse proxying |
| Production TLS identity / mTLS / reload | Experimental, Rust-first | H3 keeps a separate QUIC identity |
| Async Python substrate (`AsyncServer`, ASGI fixture) | Experimental, H1-only | Fixture only, not a maintained server |
| HTTP/2 transport | Experimental, opt-in | Plan 191 gaps stand (browser/platform evidence, trailer-scope determinism, stream-local reset hook) |
| HTTP/3 transport | Experimental, opt-in | Plans 192–195 blockers stand (`h3#338` unfixed, `#262` remainder, two-family/browser/adversarial/impairment/platform evidence missing) |
| Plan 205 observer/event/timing API | Deferred (not provided) | Plan 181 `OpsContext` stays the boundary |

Support status differs between the Rust runtime and the Python consumer by
design (Python is H1-only; advanced TLS/listener surfaces are Rust-first);
this is stated in `docs/library-capability-matrix.md` and `docs/python-api.md`.

## API changes / migration (Track E + J)

- Public-API review for this closure: `response::BoxBodyInner` is
  `pub(crate)` (no concrete-body leak); the only Hyper names in public
  stable signatures are the two intentional adapters
  (`RequestHead::try_from_hyper`, opaque-body `to_hyper_response`);
  `#[non_exhaustive]` is applied to `HttpVersion`,
  `RequestCancellationReason`, `RequestBodyError`, `ServerError`,
  `ConnectionOutcome`, `TunnelError`, and related growth-expected types;
  no accidental public re-exports were introduced by the Plan 206 moves
  (re-export shims preserve paths).
- No version bump is taken in this closure change. The next release line
  remains the documented pre-1.0 `0.1.x` → `0.2.0` transition (outbound
  opaque-body adapter per `docs/migration-guide.md`); release preparation
  publishes it as `0.2.0+` with synchronized Rust/Python metadata per Plan
  182 ownership. Current tree version stays `0.1.2` until that release prep.

## Protocol / platform matrix (Track C + I)

- H1: supported on Linux; macOS arm64 + Windows adversarial FS suites are
  manual via `platform-qualification.yml`.
- H2/H3: experimental; external two-client/browser/adversarial/impairment/
  cross-platform/soak evidence stays manual and fail-closed
  (`scripts/qualify-http2.sh`, `scripts/qualify-http3.sh`,
  `EGGSERVE_REQUIRE_*` flags).
- Routine gates executed for this closure: conformance-matrix script,
  release-metadata script, `cargo fmt --check`, MSRV `cargo +1.88 check`
  (default / `http2,tls` / `http3,tls`), stable clippy + workspace tests,
  excluded-crate check, per-feature clippy/tests (`tls`, `http2,tls`,
  `http3,tls` for core/bin as in CI), plus local `tower`/`http-interop`
  compile+test spot checks (not routine CI gates).
- Python wheel: routine CI job (`test-python-wheel.sh` on 3.14/Linux);
  installed-wheel evidence for other interpreters/platforms is manual
  release activity.

## Security review (Track D)

Focused review of the Plan 197–204 attack surfaces, verified against the
tree for this closure:

- No route around canonical framing/final privacy: services never emit
  `Transfer-Encoding`/framing headers; `normalize_response` is idempotent
  and the runtime is the sole framing authority; `ServiceError::rejected`
  keeps sanitized fixed bodies (`HEAD`/body-forbidden empty).
- Attacker-controlled metadata bounded before expensive work: target
  (414), headers (431), trailers (count/byte `TrailerLimits`), interim
  (count/byte `InterimLimits`), tunnel handshake (32 headers), PROXY
  preamble (107B v1 / ≤1024B v2, 5s timeout), SNI/roots/CRLs/chain bounds.
- No identity forgery: forwarding headers stay untrusted by default and
  populate provenance-tagged effective fields only under explicit trust;
  raw peer preserved; canonical Host/target never rewritten; TLS identity
  selection performs no IO and key/cert bytes are never logged.
- Long-lived resources owned: per-connection/stream admission, body and
  response no-progress timeouts, tunnel single-owner 32 KiB backpressure,
  downstream admission split (`max_in_flight_requests` pre-response only),
  Python 16-chunk bridges with `max_async_tasks`; shutdown is
  level-triggered/idempotent and lifecycle cancellation wakes bridges.
- Supply chain: `cargo audit` + `cargo deny check` executed for this
  closure (see verification below).

Residual/manual items (not blockers for closure tiers): independent
adversarial-wire review of H2/H3 paths, cross-platform PROXY/TLS runtime
evidence, and 24h soak are release-qualification activities owned by the
manual workflows, marked blocked where unavailable rather than passed.

## Feature / dependency graph (Track F)

Supported combinations (verified to compile; routine CI gates the starred
ones): default/minimal H1*, `tls`*, `http2`+`tls`*, `http3`+`tls`* (pulls
`tls`/QUIC by design), `http-interop`, `tower` (implies `http-interop`),
all qualified Rust features, Python wheel feature set. Minimal H1 pulls no
H3/Quinn/Tower/Python-async dependencies; MSRV 1.88 holds for the gated
sets. Nonsensical combinations (e.g. `http3` without its required TLS/QUIC,
H3-over-Unix) are rejected or fail closed, not supported.

## Documentation closure (Track G)

This change synchronizes: `docs/extension-contract.md` (tunnel implemented,
H2/H3 opt-in experimental — obsolete Plan 176-deferred prohibitions
removed), `docs/public-api-boundary.md` + `docs/api-stability.md` (Plan 199
supersedure; experimental rows for trailers/interim/tunnel/proxy/TLS-identity/
adapters/async-Python), `plans/ROADMAP.md` (tunnel successor + Plan 205
deferral recorded), `README.md` (closure tiers pointer),
`AGENTS.md` + skill (205 deferred / 208 closed, kept consistent; skill
symlink covers `.agents/`), `architecture/overview.md` (already truthful;
no change), `docs/non-goals.md` (already reflects Plans 198/199/204;
obsolete 172/176/183 prohibitions absent — no change),
`docs/downstream-app-server.md` and `docs/library-capability-matrix.md`
(already inventory Plan 207; no change).

## Examples / fixtures (Track H)

Canonical examples remain small, loopback-bound, safe-by-default and are
covered by `scripts/test-examples.sh`: native custom/streaming/application
services, caller-owned stream, Tower via `tests/interop_http_tower.rs`
(fixture, not a second example surface), prebound/systemd/proxy/advanced-TLS
via their suites + `https_server.rs` (single-identity), async Python via
`examples/python_async_server.py`, ASGI via the test fixture only, tunnel
via `tunnel_upgrade.rs` echo + `tokio-tungstenite` fixture (no WS codec in
core). No new examples added: advanced surfaces stay suite/fixture-owned to
avoid teaching trust-check disabling.

## Verification (this closure)

Routine CI sequence executed locally before push (commands in
`.github/workflows/ci.yml`): conformance-matrix PASS, release-metadata
PASS, `cargo fmt --check` PASS, MSRV checks PASS, stable clippy + workspace
tests PASS, excluded-crate check PASS, per-feature clippy/tests
(`tls`, `http2,tls`, `http3,tls`) PASS, supply-chain audit/deny PASS, plus
local `tower`/`http-interop`/`all-features` check spot PASS. Python wheel
job runs in remote CI (Linux/3.14); local Python expectations recorded in
the push thread.

## Deferred follow-ups (explicit, with rationale)

1. Plan 205 observability extension — deferred (no consumer; see table).
2. H2 supported-tier promotion — needs a new scoped plan closing the Plan
   191 gaps; Plan 191 is not an open authority.
3. H3 supported-tier promotion — needs a new scoped plan closing the Plan
   192 `BLOCKED` items plus 193-evidence gaps; Plans 193–195 are not open
   promotion authorities.
4. `0.2.0` release prep (metadata sync + notes + wheels/crates publish) —
   manual release activity, not this closure.
5. macOS/Windows platform qualification + installed-wheel matrix — manual
   workflows, marked blocked where not executed.

## Handoff

Downstream projects can build a Rust application server or a maintained
Python ASGI server on EggServe without importing internals or reproducing
transport/security machinery, within the tiers above. Future EggServe work
is driven by concrete downstream gaps, not framework feature accumulation.
