---
name: eggserve-dev
description: Use when working on eggserve code, plans, docs, or architecture. Covers Rust workspace conventions, plan-driven development, CI validation, security policy, and the layered crate layout.
---

# eggserve Development Skill

`AGENTS.md` at the repository root is the canonical agent-facing index (CI
commands, quirks, architecture/docs index). This skill adds working detail;
keep the two consistent.

## Project identity

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. Static
serving is the primary product; a separate downstream project may use the
qualified HTTP-only Rust substrate, but EggServe itself is not an application
server. The
CLI is static-only; the Python facade also supports bounded synchronous custom
handlers; `eggserve.lowlevel` exposes a handler-only runtime/service substrate
(`RuntimeConfig`/`Server`, `Response.stream`, `StaticResponder` composition)
plus the experimental H1-only async substrate (Plan 204: `AsyncServer`/
`AsyncRequest`/`AsyncBody`/`AsyncResponse`/`Tunnel`, manual asyncio bridge,
ASGI test fixture only)
for downstream bounded application servers; and `eggserve-core::server` exposes
an experimental, low-level Rust service boundary. EggServe is not an application
framework, ASGI/WSGI runtime, CGI executor, FastCGI gateway, proxy, or
general-purpose `socketserver` replacement. Plan 167 closed as no-go: no
in-tree CGI/FastCGI adapters; downstream gateways implement the canonical
`Service` trait. The `server` module remains experimental even though the HTTP
bridge is qualified by Plan 175. Plan 199 implements generic tunnel/upgrade/Extended CONNECT, superseding deferred Plan 176: one-shot transport-backed `TunnelCapability` (H1 `Upgrade`, `CONNECT`, H2/H3 Extended `CONNECT`; H3 generic `:protocol` blocked by `h3` 0.0.8), `accept` returns a handshake `Response` (`101` H1 / `200` otherwise, runtime owns framing, no raw socket) plus bounded single-owner `TunnelIo`; denial stays ordinary HTTP; WebSocket framing stays downstream (see `tunnel_upgrade.rs`). Plan 216 moves tunnel authority to the direct crates without a new service model: neutral intent vocabulary lives once in `eggserve-primitives::tunnel` (Hyper/Tokio-free); transport execution lives once in `eggserve-server::tunnel`; compatibility H1/H2 validate through the shared helpers and run the shared future (`server/connection/tunnel.rs` deleted; H3 stream bridging lives once in `eggserve-h3` (Plan 220)). Plan 217 makes `eggserve-server::Service` the single contract for direct H1 and compatibility H2 plus a downstream fixture. Direct services use additive `Service::call_with_tunnel` (default drops → ordinary denial); handlers own only IO (`FnOnce(TunnelIo)`, capture the lifecycle for cancellation). Plan 200 implements optional `http`/`http-body`/Tower interop (`http-interop` → `primitives::interop`, `tower` → `server::tower`; per-request Tower clones, no shared mutex; see `docs/http-interop.md`). Plan 201 implements listener ownership and process-manager integration: `from_std_listener` / `from_unix_listener` / `from_std_unix_listener` / `from_systemd_index` / `from_systemd_name` / `http3_socket(std UdpSocket)` feed the single `accept_loop_multi` (stable `tcp-0`/`unix-0` IDs via `ServerHandle::endpoints()`); Unix is plaintext with no unlink and truthful `None` endpoints; systemd validates `SOCK_STREAM` + listening + family over `rustix` `net`; H3 prebound UDP validates same-port TCP+UDP.

**Not** a general web server, framework, ASGI/WSGI runtime, or Granian replacement. Plan 205 (application observability hooks) is explicitly deferred by Plan 208 — no `RequestObserver`/request-ID/lifecycle-event extension; the Plan 181 per-runtime `OpsContext` remains the observability boundary. Plan 208 closes the Plan 196 program with H1 + canonical `primitives` supported and `server`/H2/H3/tunnel/trailer/adapter/listener/proxy/TLS-identity/async-Python remaining experimental (see `release/plan-208-foundation-release-closure.md`).
Plan 215 makes `eggserve-server` the implementation home of the mature H1
connection runtime (ops, error taxonomy, response policy, shared limit
authority, service contract shape, connection vocabulary, H1 config/state,
H1 driver, Hyper conversion boundary; Plan 249: single H1 authority with
`Auto` classified before Hyper, H2-only core execution, structured
per-connection shutdown) with a 16-scenario
direct-vs-compatibility parity suite
(`crates/eggserve-core/tests/direct_h1_parity.rs`) and topology-gate
ownership rules; unified `Service` identity and tunnel acceptance are
explicit Plan 216 input; Plan 217 finishes convergence with a downstream fixture (see `release/plan-215-direct-runtime-parity.md`). Plan 220 moves the H3 adapter into `eggserve-h3`.
Plan 219 collapses static/path/filesystem authority onto `eggserve-static`
(sole owner of path parsing, secure-root resolution, filesystem confinement,
MIME, and response planning); `eggserve-core` keeps those paths as
compatibility facades with no second resolver, proven by the authority
conformance fixture
(`crates/eggserve-core/tests/static_authority_conformance.rs`). Plan 221 makes
the first-party frontends prove the direct architecture: `eggserve-bin` and
`eggserve-python` name the leaf crates directly for neutral paths, with
binary unit tests driving leaf `Server` + leaf `StaticService`; extended
orchestration plus the confirmed-used `eggserve_bin::run_cli` CLI stay
compatibility-owned as documented orchestration under the Plan 225 facade
closure (classified inventory, no second authority; removal needs a separate
migration plan). Plan 224 closes as NO-GO: no
`eggserve-capfs`/`eggcapfs` crate is created; `eggserve-static` remains the
single path/filesystem confinement authority (resolver consumes
`ConfinedPath`/`StaticPolicy`, returns `BodySource` with MIME planning,
duplicates parse-level validation as defense in depth, isolates production
unsafe to `fs/windows.rs`, has no second consumer; see
`release/plan-224-capability-filesystem-evaluation.md`).

Plans 243–247 complete the maintainability convergence that follows the
authority split: `eggserve-server` uses a durable shutdown signal and drains
runtime-owned connection tasks; compatibility H1 entry points project into the
direct runtime while H2/TLS/proxy composition remains in core;
`eggserve-static::StaticService` owns static request planning/rendering and core
retains a wrapper; Python publishes `lowlevel.pyi` and `py.typed` with a
dedicated PyO3 registration module; and the topology gate rejects orphan Rust
sources while documenting accepted inert compatibility features. The closure
record is `release/plan-248-maintainability-convergence-closure.md`.
Plans 249–250 close the post-closure corrective: compatibility `Auto`
classifies before any Hyper service exists (every H1 path delegates to the
single direct authority; core executes H2 only) and per-connection shutdown
is structured under the connection task with no detached forwarder; see
`release/plan-250-h1-authority-lifetime-corrective-closure.md`.

Plans 251–256 close the post-convergence maintenance campaign with no
public API, capability, or support-tier change: Python stubs match runtime
shapes (`AsyncRequest` dict headers, text `*_addr` vs tuple `*_address`,
supported subclass hooks; strict installed-wheel fixture plus runtime shape
tests); every core/server connection overlap is classified (sharing needs a
new public transport API, so parallels stay crate-private, H2-gated, and
topology-guarded — see the ledger in `architecture/crate-topology.md`);
async producers wait bounded for the first native pull (HEAD/body-forbidden
never advance application state); broad unused-import suppressions are gone
(clippy is the authority; H2/test-only uses are precisely gated); the
topology gate adds the overlap rule plus a `--self-test` mutation suite;
Rust consumers pick the direct H1 leaf profile or the compatibility
multiprotocol profile (see `release/plan-256-post-convergence-maintenance-interop-closure.md`).

Plan 212 extracts the reusable server-side TLS identity, SNI, WebPKI
client-auth, trust/CRL, and reload substrate into the neutral `eggnet-tls`
crate. `eggserve_core::tls` remains a compatibility re-export; EggServe retains
only transport-facing Tokio TLS, while the opt-in H3 adapter consumes the
isolated `eggserve-h3` boundary. The neutral crate has
no EggServe/Eggress/EggFetch/application or transport dependency (see
`architecture/eggnet-tls.md`).

## Workspace layout

Seven workspace crates plus one excluded Python packaging crate:
- `crates/eggnet-tls/` — neutral rustls identity, trust, client-auth, and reload substrate
- `crates/eggserve-primitives/` — canonical application values with small
  transport-neutral dependencies
- `crates/eggserve-server/` — mature H1 connection runtime and transport
  boundary (Plan 215 implementation home; Plan 216 tunnel authority direct; Plan 217 single Service contract, H2 as transport glue)
- `crates/eggserve-static/` — sole static/path/filesystem authority
  (`SecureRoot`/capabilities, `path`, planner, MIME; Plans 219 + 224 NO-GO)
- `crates/eggserve-h3/` — experimental H3/QUIC transport adapter (Plan 220 authority)
- `crates/eggserve-core/` — compatibility and composition umbrella preserving the mature API (facades, adapters, documented orchestration; Plan 225 closure)
- `crates/eggserve-bin/` — binary: CLI, accept loop, signal handling (Plan 221: neutral paths on leaf crates; extended orchestration compatibility-owned under Plan 225)
- `crates/eggserve-python/` — Python wheel packaging (maturin + PyO3 0.29.2, Plan 221: neutral bridge on leaf crates + confirmed-used `eggserve_bin::run_cli`; excluded from workspace; packages the native extension and extension-backed CLI, with no separate bundled executable)

Other directories: `architecture/` (deep-dive docs), `docs/` (reference docs),
`plans/` (historical design/implementation records plus the `ROADMAP.md` and
`RELEASE-READINESS-ROADMAP.md` roadmap files),
`examples/` (canonical CLI/Python examples plus Cargo examples and tiny
fixtures), `fuzz/`, and `scripts/` (small fast/full/deep verification hierarchy
plus package/release checks). The example index is `examples/README.md`.

## Non-negotiables

1. **Safe defaults** — loopback bind, no symlinks, no dotfiles, no directory listing. Every unsafe behavior requires explicit opt-in via CLI flag.
2. **No serving outside root** — path traversal and symlink escape denied at library level. On Unix with safe defaults, descriptor-relative traversal via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`.
3. **No broad dependencies** — every dependency must have an explicit purpose. See `docs/dependency-policy.md`.
4. **Plan-driven development** — every change must be traced to a plan in `plans/`. No ad-hoc feature additions.

Keep detailed Python deviations in `docs/python-http-server-compatibility.md`
and detailed Rust ownership in `architecture/runtime.md`; plans record change
history and are not prerequisites for understanding current behavior.

## CI validation sequence

Routine CI runs three concurrent jobs (`rust`, `supply-chain`, `python`):

```sh
# rust job
python3 scripts/verify-conformance-matrix.py                # corpus/matrix + Plan 207 app-server inventory gate (runs first!)
python3 scripts/check-crate-topology.py                     # Plan 211–247 ownership/topology, facade, orphan-source, and feature gate
python3 scripts/check-python-release-metadata.py            # version + [profile.dist] sync (cheap, before builds)
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings  # lint (warnings are errors)
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked  # excluded crate still parses
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # TLS lint
cargo test -p eggserve-bin --features tls                   # TLS tests
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls

# supply-chain job: install-cargo-tools.sh, check both lockfiles
# python job: bash scripts/test-python-wheel.sh
# preflight re-runs check-python-release-metadata.py, then
# builds wheel with maturin, installs in venv, runs smoke + tests
```

Manual platform qualification is separate from routine CI:

```sh
gh workflow run platform-qualification.yml --ref main
```

It exercises the installed wheel on macOS arm64 and the Windows adversarial
filesystem suites. The Windows suite explicitly skips the two cases where
NTFS rejects an external path-based root rename while a descendant handle is
open. Keep Windows support language aligned with the evidence in
`docs/toolchain-support.md` and `docs/security-review.md`.

Or use the local verification script:

```sh
./scripts/verify.sh fast                 # routine dev check (Rust workspace + Python crate check)
./scripts/verify.sh full                 # pre-release validation (examples, Rust + Python wheel)
./scripts/verify.sh deep                 # expensive suites (manual)
bash scripts/qualify-http2.sh             # manual Linux H2 wire/ALPN qualification
bash scripts/qualify-http3.sh             # manual H3/QUIC qualification; direct clients required for wire evidence
```

### Supply-chain and optional package checks

The routine supply-chain job and release preflight check both distributed
dependency closures. Run the same checks locally when preparing a release:

```sh
bash scripts/install-cargo-tools.sh     # deterministic audit/deny installation
bash scripts/check-supply-chain.sh     # root + excluded Python audit/policy
bash scripts/verify-cargo-packages.sh --mode all  # package dry-run gates
```

The package dry-run remains manual release validation. The excluded Python
crate keeps its own lockfile, so never replace the shared script with a root
only `cargo audit` or `cargo deny check` invocation. A scheduled daily
workflow (`.github/workflows/advisory-scan.yml`, Plan 218) re-runs the same
gates without a push/PR. Both lockfiles are distributed security boundaries;
direct `rustls` constraints carry a `0.23.45` caret floor (RUSTSEC-2026-0285)
in every constraining manifest including the excluded Python crate — never
roll it back. `deny.toml` denies wildcard requirements and bans
`native-tls`/`openssl-sys` for the rustls/ring-only stack (see
`docs/dependency-policy.md`).

## Key conventions

- **Plan 212 neutral TLS** — `eggnet-tls` owns bounded PEM parsing, key/cert
  pairing, SNI, explicit WebPKI client-auth modes, trust/CRL bounds, and atomic
  reload snapshots. Its production graph contains only `rustls` and
  `rustls-pki-types`; it must not gain Tokio, HTTP, QUIC, proxy, tracing,
  EggServe, Eggress, or EggFetch dependencies. `eggserve_core::tls` re-exports
  it for compatibility; keep HTTP/3-specific QUIC dependencies behind the
  `eggserve-h3` boundary and the core `http3` feature.

- **Plan 222 cross-repo TLS consolidation (eggserve side)** — `eggnet-tls` adds
  the neutral ALPN hook (`alpn_protocols` / `load_tls_config_with_alpn`,
  16×255 bounds via `TlsError::InvalidAlpn`, last-wins against the HTTP-only
  `http2` convenience) so non-HTTP transports never reuse `http_alpn_protocols`.
  Never roll back the `0.23.45` rustls caret floor (RUSTSEC-2026-0285) in any
  constraining manifest including the excluded Python crate; sibling eggress/
  eggfetch floors are still bare `0.23` (locks at .45) and are follow-ups in
  those repos. Eggress server migration (its optional-mTLS branch is missing
  `allow_unauthenticated()`) and the eggfetch no-dependency evaluation live in
  those repos; `eggnet-tls` stays published from this workspace as a versioned
  crates.io package, never a git dependency (see `architecture/eggnet-tls.md`).

- **Plan 223 cross-repo CONNECT consolidation (eggserve side)** — eggserve
  owns only inbound server-side `CONNECT`/tunnel acceptance (Plans 199/216:
  neutral vocabulary in `eggserve-primitives::tunnel`, execution in
  `eggserve-server::tunnel`); it has no outbound H1 CONNECT encoder/parser
  and must never acquire the neutral CONNECT crate, an HTTP client stack,
  or an eggfetch/eggress product dependency. The shared outbound
  caller-owned-stream wire primitive (authority/request-head encode,
  bounded response-head parse, read-ahead preservation; dialing/DNS/TLS/
  timeout/retry/routing/lifecycle caller-owned) plus both product
  migrations live in those repos; eggress inbound `handle_connect`/auth/
  forwarding/relay stays locally owned there (see `plans/223-http-connect-cross-repo-consolidation.md`).

- **Plan 214 extraction parity** — `eggserve-primitives` owns the extracted
  canonical request/response/body/lifecycle model with only small
  transport-neutral dependencies; `eggserve-server` owns the mature direct
  generic H1 connection runtime (ops, error taxonomy, response policy,
  shared limit authority, service contract shape, connection vocabulary, H1
  config/state, H1 driver, Hyper conversion boundary) and cannot depend on
  core/static; and
  `eggserve-static` owns the extracted descriptor/handle-relative resolver,
  planner, MIME behavior, and static service, and (Plan 219) is the sole
  implementation owner of static path parsing, secure-root resolution,
  filesystem confinement, resolved capabilities, MIME selection, and response
  planning — `eggserve-core` keeps `src/fs`, `src/path`, `src/mime.rs`
  deleted with re-export facades only (`ServeState` retains a `SecureRoot`;
  capability bridge forwarded via `python-bindings-internal`), proven by the
  authority conformance fixture
  (`crates/eggserve-core/tests/static_authority_conformance.rs`). `eggserve-core` is the
  compatibility and composition umbrella for Python and advanced protocol paths and exposes
  direct crates under `eggserve_core::layers`. Do not introduce simplified
  parallel runtimes or pathname check-then-open fallbacks. The
  direct-vs-compatibility H1 parity suite
  (`crates/eggserve-core/tests/direct_h1_parity.rs`) plus the topology gate
  own the boundary; keep H2 wire mechanics/TLS-compat work in
  core as explicit transport glue with parity evidence (Plans 216/217 landed with the direct-service convergence fixture).
  (Plan 216 tunnel: neutral vocabulary in `eggserve-primitives::tunnel`,
  execution in `eggserve-server::tunnel`; Plan 217 single Service contract + downstream fixture;
  handlers own only `TunnelIo`, capture the lifecycle.)
  Run
  `scripts/check-crate-topology.py` after graph changes; see
  `architecture/crate-topology.md`.
- **Plans 243–247 convergence** — preserve durable direct-server shutdown and
  JoinSet draining, keep compatibility H1 delegation on the direct driver,
  keep static service behavior in `eggserve-static`, and keep Python typing and
  registration maintenance at the wheel/native boundary. Run the topology gate
  after module or feature changes; its orphan-source walk and inert-feature
  assertions are part of the supported CI boundary. Plan 249: compatibility
  `Auto` classifies before any Hyper service exists, core executes H2 only,
  and per-connection shutdown is structured (`run_with_connection_shutdown`);
  the topology gate rejects core Hyper H1 machinery and detached forwarders.
- **Manual argument parsing** in `args.rs` — no clap dependency. The CLI grammar
  is `[OPTIONS] [PORT] [DIRECTORY]`; positional parsing owns those two logical
  slots, treats a directory after an occupied port slot verbatim (including a
  numeric name), and rejects excess positionals. A host-only `--bind` leaves
  the port slot available; `--directory` occupies the directory slot.
- **Two DotfilePolicy types** — `eggserve_static::path::DotfilePolicy` (parsing, facaded as `eggserve_core::primitives::PathDotfilePolicy`) and `policy::DotfilePolicy` (serving). Both must agree.
- **eggserve-python excluded from workspace** — has its own Cargo.lock, built via maturin. Don't run `cargo test --workspace` for Python crate.
- **Frozen Python classes** — `#[pyclass(frozen)]` and `frozen=True` dataclasses
- **`#[allow(dead_code)]` on public API types** — consumed externally (Python bindings)
- **Error taxonomy** — Five distinct error types: `PathRejection` (17 variants, path validation), `RequestValidationError` (6 variants, HTTP-level, Python-facing), `ServerError` (10 variants, server lifecycle, `#[non_exhaustive]`), `ServiceErrorKind` (private kind enum behind the public `ServiceError` struct; 4 kinds: `Internal`, `Rejected(u16)`, `Panic`, `Timeout`; struct stays future-proof, inspect via `is_panic`/`is_timeout`), `RequestBodyError` (14 variants, body consumption incl. `InvalidTrailers`/`TrailersNotReady`, `#[non_exhaustive]`). `RequestCancellationReason` + `ConnectionOutcome` are also `#[non_exhaustive]` (match with wildcard). See `architecture/error-taxonomy.md`. Never synthesize a second HTTP error after final commitment.
- **Plan status** — Plans are historical change-trace records. The current product and compatibility contract is owned by `README.md`, `docs/python-http-server-compatibility.md`, and the relevant architecture pages. Production servers use the shared `RuntimeState` admission pool.
- **Canonical HTTP types (stable)** — `Method`, non-exhaustive `HttpVersion` (`Http10`, `Http11`, `Http2`, `Http3`), `Authority`, `HeaderBlock`, `HeaderValue` (octet-preserving; `from_bytes`/`as_bytes()`, fallible `to_str()`; `OWS` stripped), `HeaderValueTextError`, `RequestTarget` (`raw_bytes()`/`path_bytes()`/`query_bytes()`; empty query → `None`), `RequestHead` (including validated effective authority), `RequestLifecycle`/`RequestCancellationReason` (`#[non_exhaustive]`, Plan 174 disconnect observer), `RequestContext` (single attachment point: `connection()` + `lifecycle()` + bounded `interim()`), `ConnectionInfo` (`local_addr`/`remote_addr` raw preserved as `Option<SocketAddr>`; non-socket `None` via `SocketEndpoints`/`without_socket_addrs`; Plan 202 effective layer `proxy_source`/`proxy_destination`/`proxy_provenance` + `effective_client`/`effective_scheme`/`effective_authority`/`forwarded_provenance` with `effective_client_addr()`/`has_trusted_proxy_metadata()`), `ProxySourceKind` (`proxy_v1`/`proxy_v2`/`forwarded`/`legacy_forwarded`), `TrustedProxyConfig`/`IpPrefix`/`ProxyProtocolConfig`/`ForwardedConfig` (explicit peers/CIDRs, no DNS/implicit loopback; Unix explicit `trust_unix`; PROXY disabled 5s timeout; forwarded disabled 4 KiB/16-element budgets), `StatusCode`, `ResponseHead`, `ResponseBody` (`Empty`/`Bytes`/`File`/`Stream`/`EmptyWithLength`), `Response`, `BodyLength` (`Known`/`Unknown`), `ResponseStream`/`ResponseStreamError` (`with_trailers`/`with_known_length_and_trailers`), `Trailers`/`TrailerLimits`, `InterimSender`/`InterimLimits`, `normalize_response()` are all stable. Python stdlib-shaped surfaces stay text-only (opaque headers omitted, not coerced). HTTP/2 and HTTP/3 wire support are opt-in via the experimental `http2` and `http3` features.
- **Plan 184 request/runtime seams** — `RequestTarget::parse()` is the sole HTTP target-form classifier; runtime/static code hands its validated path to `ConfinedPath::from_path_component()`. Body-policy branches converge on one service admission/invocation kernel. Typed `LifecycleDisposition` state is mapped to HTTP/1 headers only by the HTTP/1 adapter. H1 fields project onto the direct authority via `RuntimeConfig::direct_h1_config()` with no second defaults table (`server/config/http1.rs` is a retained inventory placeholder with no H1 authority, Plan 249). H1 runs with `.with_upgrades()` + H2 `enable_connect_protocol()` + H3 `enable_extended_connect(true)` for validated tunnel handshakes (Plan 199); ordinary services pay no upgrade cost. The workspace MSRV is Rust 1.89; keep dependency updates compatible with the pinned MSRV and supply-chain checks.
- **Plans 189–190 multiprotocol correctness/qualification** — H1 Reject remains framing based; H2 Reject uses Hyper `Incoming::is_end_stream()` and H3 performs one bounded receive probe when `Content-Length` is absent/zero, so DATA cannot reach a Reject service. Focused H2/H3 tests cover DATA without `Content-Length`, bodyless dispatch, H3 zero-length-plus-DATA, bounded probe timeout, sibling isolation, detached lifecycle wake-up, (Plan 192) early-error stream scoping plus close-race survival, and (Plan 194) stalled H3 producer timeout with sibling survival. Shared `RequestBody` consumption checks both under- and over-declared lengths. H3 reuses the connection lifecycle registry for peer/shutdown cancellation and stream-local failure handling. Runtime error status/reason/body construction is canonical across transports. H2 response timeout accounting observes application-body producer/poll progress only (no guaranteed wire progress, no public Hyper stream reset; stall uses connection fallback). H3 observes per-stream producer + send progress with stream-scoped reset (siblings survive). Both remain experimental. See `release/plan-190-multiprotocol-corrective-qualification.md`.
- **ResponseStream producer bound** — `ResponseStream::new`/`with_known_length` accept `Stream<Item = Result<Bytes, ResponseStreamError>> + Send + 'static`; `Sync` is intentionally not required. The stream is one-shot and exclusively polled by its owning connection task. `Response` and the internal transport body remain `Send`, while concurrent body polling is unsupported. Plan 198 adds `with_trailers`/`with_known_length_and_trailers` for one terminal trailer block (no data after, `HEAD`/body-forbidden never poll, known length counts data only, adapters map without buffering).
- **Canonical response semantics** — `StatusCode` accepts 100–599 only; 1xx/204/205/304 are body-forbidden (only 304 may retain a matching representation `Content-Length`); weak metadata ETags may satisfy `If-None-Match` but never `If-Range`; exactly 0/1 authoritative `Date` per `DatePolicy` at final construction (`SystemClock` default = one `Date`; `Suppress` = zero; Hyper `auto_date_header(false)`). `normalize_response` maps every body-forbidden status except 304 to `BodyLength::Known(0)` and drops HEAD/body-forbidden streams without polling (prompt producer release). `BodyLength::Unknown` never becomes `Content-Length: 0`; unknown HEAD omits the header. Normalization is idempotent (`Response::is_normalized`, mutation via `head_mut`/`take_body` clears it) so the static service can normalize eagerly while the connection pipeline normalizes every service response. The runtime is the only framing authority for `Content-Length`/`Transfer-Encoding`/reuse. Python callback conversion stages headers and body ownership atomically; malformed body state never falls back to an empty response.
- **Public Hyper boundary** — Canonical application-facing request/response types, `Service`, and the caller-owned connection driver are Hyper-free. `eggserve_server::adapters::to_hyper_response()` is the direct low-level adapter (the compatibility `primitives::to_hyper_response()` delegates to that single authority over the same canonical types (Plan 217 identity, no second framing implementation); parity covered by the H1 suite plus the direct-service convergence fixture plus both tunnel suites); the outbound adapter returns an opaque `http_body::Body` and must not expose `BoxBody`/`UnsyncBoxBody` as a stable contract. The `0.2.0` line carries the documented pre-1.0 `0.1.x` → `0.2.0` transition (Plan 226 executed the version move; never publish this line as `0.1.x`).
- **Canonical response normalization** — All response producers converge on `primitives::canonical::normalize_metadata()`.
- **Plan 165 response privacy** — `RuntimeConfig.response_policy: ResponsePolicy` owns `server_identification` (`None` suppressed default; `builder.server_header(..)` / `config.server_header_value()`), `date_policy` (`SystemClock` default, `Custom(provider)` trusted time, `Suppress` RFC tradeoff), `stripped_response_headers` (validated denylist, post-service, no framing/`date`/`content-range`, `minimal_fingerprint()` strips `x-powered-by`), `error_policy` (`Minimal` fixed bodies default, `Empty` runtime-errors-only; app `Ok` never rewritten). `StaticPolicy.static_metadata` (`standard()` vs `minimal_fingerprint()`; planner `plan_file_response_with_preconditions_and_metadata`). `ServeConfig.error_policy` transferred by `try_from_serve_config`. CLI keeps standards defaults; Python `lowlevel` exposes the safe subset (`server_header`, `system`/`suppress`, denylist, `minimal`/`empty`) with `Custom` clocks Rust-only.
- **`server` module types** — `eggserve-core::server` provides the runtime service boundary for embedding. The module is experimental; API may change.
- **Plans 186/190/191 H2 qualification** — The opt-in Rust `http2` path is experimental. Deterministic H2+TLS tests, targeted Reject-body regressions, Linux `curl` wire qualification, two-family interop (curl/libnghttp2 plus python-h2), h2spec classification, and flow-control/GOAWAY/load evidence pass; browser/platform evidence, trailer-scope determinism, and a public safe per-stream reset/wire-progress hook remain release gaps. Use `scripts/qualify-http2.sh` and `release/plan-191-http2-supported-tier-qualification.md` for the current qualification boundary.
- **Plans 187–190/192–195 H3 boundary** — The opt-in Rust `http3` path remains experimental. It uses optional `h3`/`h3-quinn`/`quinn` dependencies; `ServerBuilder::http3_identity` supplies PEM paths for a separate TLS 1.3/`h3` QUIC config; startup binds UDP to the resolved TCP port, shares connection/service/file budgets, disables 0-RTT, and uses canonical request/response/body adapters. Plan 190 closes the deterministic DATA/bodyless/probe-timeout/lifecycle corrections and preserves the evidence-sensitive qualification scripts, but this environment lacks independent H3 clients, adversarial wire testing, and cross-platform H3 runtime evidence. Plan 192 adds early-error receive aborts plus early-error scoping and close-race regressions, and closes `BLOCKED` on the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11): upstream `hyperium/h3#338` has no released fix and the `#262` stream-drop remainder is unresolved. Plan 193 closed at preflight on 2026-09-10 without entering promotion qualification (unmet Plan 192 prerequisite; unchanged candidate; `#338`/`#262` still open; two-family, browser, adversarial, impairment, and platform evidence inventoried as unavailable). Plan 194 bounds the H3 `ResponseStream` producer poll with `response_write_timeout` no-progress semantics (absolute deadline, empty chunks are not progress, per-stream reset, `WriteStallTimeout` observed, siblings survive) and corrects the promotion-trace docs; tier unchanged. Plan 195 correctively qualifies that bound (H3 suite 14 → 16: shutdown-race drain plus write-stall observability/permit-release regressions) with no source change; tier unchanged. H3 remains experimental; a future promotion requires a new scoped plan. See `release/plan-193-http3-supported-tier-qualification.md`, `release/plan-194-http3-producer-timeout-correction.md`, and `release/plan-195-http3-response-timeout-corrective-qualification.md`.
- **Transport-neutral driver** — `eggserve_server::connection::serve_http1_connection` is the single mature H1 entry (strict HTTP/1 over any `AsyncRead + AsyncWrite` stream, no Hyper types, no fabricated addresses); the direct `Server` (bind + `from_listener`/`from_std_listener` prebound TCP, accounted accept loop, per-connection shutdown relay, `wait()`/`ops_snapshot()`) shares the same driver and `RuntimeState`. `eggserve_core::server` keeps the tunnel-capable H1 facades (delegating to the direct authority) plus feature-gated `serve_http_connection` (bounded H1/H2 prior knowledge, `Auto` resolved before any Hyper service exists; core executes H2 only). Per-connection shutdown is structured under the connection task (`run_with_connection_shutdown`, no detached forwarder). Direct tunnel authority (Plan 216) means H1/H2 validation runs through the shared neutral helpers with the shared `run_tunnel` future; H2 dispatches through the same canonical contract as direct H1 (Plan 217, explicit transport glue). Both drive a canonical `Service` with explicit `ConnectionContext`, shared `Arc<RuntimeState>` (`RuntimeState::try_new(&config)` preferred, `new(&config)` validates + panics), and per-connection `ConnectionShutdown` returning `ConnectionOutcome`. Invalid hand-constructed `RuntimeConfig` is rejected at `ServerBuilder::build()`, `RuntimeConfig::validate()`, `RuntimeState::try_new()`, and the caller-owned entry (logs + `Internal`) before semaphore/Hyper use. `ConnectionShutdown` is level-triggered and idempotent (pre-signaled shutdown observed promptly, no polling). The compatibility TCP/TLS `Server` selects H1/H2 through ALPN or the bounded prior-knowledge classifier; raw Hyper helpers are crate-private. No fabricated socket addresses, no Hyper types in the driver signature.
- **RequestBody is one-shot** — `RequestBody` can only be consumed once. The `Service` trait's `call` method takes `Request` by value. Body policy defaults to `Reject`. Plan 174: Stream bodies share Active→Complete/Abandoned/Failed lifecycle (Drop-derived for network bodies; in-memory never forces close); service may return response-start with Active body delegated, reuse waits for Complete, Abandoned/Failed forces close (Hyper-pinned). `Request::lifecycle()`/`into_parts_with_lifecycle()` expose `RequestLifecycle` (`cancelled()`, `is_cancelled()`, `cancellation_reason()`; PeerDisconnected/ServerShutdown/ConnectionTimeout/TransportFailure, first wins, `#[non_exhaustive]` — match with wildcard). Plan 198: `RequestContext::interim()` is the bounded 1xx sender (only 1xx no 101, no body/trailers, no post-commit, HTTP/1.0 suppressed, single 100); `RequestBody::trailers()`/`read_all_with_trailers()` expose terminal trailers only after completion (separate bounds, `InvalidTrailers` on failure, H1 without valid framing cannot inject). Plan 197: `RequestContext` (`connection()` + `lifecycle()`) is the single attachment point for transport metadata + future opaque capabilities (no type map, no raw handles); prefer `Request::context()`/`new_with_context()`/`into_parts_with_context()` for new code — `connection()`/`lifecycle()`/`into_parts*` forward to the context and preserve the Plan 175 path. Stream `Service::call` stays collapsed as `min(body, handler)` for compat (disambiguated via lifecycle); remaining body timeout continues after return via watchdog. `max_in_flight_requests` bounds pre-response `Service::call` only; downstream app admission is separate.
- **Downstream app-server consumer (Plan 175) + application-service contract (Plan 197)** — `crates/eggserve-core/tests/app_server_consumer.rs` is the external-consumer qualification: bounded full-duplex bridge (cap-2 channels, no `read_all`, no Hyper/private imports; fixture-local event names only), deferred ownership, lifecycle cancellation, handler/body timeout split, downstream admission split, TCP/TLS/caller-owned parity, non-gating perf sanity. `crates/eggserve-core/tests/application_service_contract.rs` is the Hyper-free stabilized-contract fixture (`RequestContext`, buffered/streamed/lifecycle, `#[non_exhaustive]` wildcards, runtime admission 503); `crates/eggserve-core/examples/application_service.rs` is the minimal native demo (no static FS). `crates/eggserve-core/tests/direct_h1_parity.rs` proves the direct `eggserve-server` H1 kernel matches the compatibility pipeline wire-for-wire (16 scenarios; tunnel excluded, Plan 216 input). `crates/eggserve-core/tests/direct_service_convergence.rs` proves one direct `eggserve-server::Service` drives direct H1 and compatibility H2 (Plan 217). Builder-facing rules + normative 7-stage commitment/cancellation + `Send + Sync` (no `poll_ready`) + error-taxonomy rules live in `docs/downstream-app-server.md`; EggServe itself is not an app server/ASGI runtime. `Service::call` stays `Response`-only (no `ServiceOutcome`; Track C decision).
- **HTTP/3 dependency isolation and qualification (Plan 213)** — `eggserve-h3` owns the direct Quinn/H3/H3-Quinn dependency set; the core compatibility adapter consumes it only behind `http3`. The no-feature graph must not contain H3/QUIC packages. Plan 220 moves the adapter implementation into `eggserve-h3` with a thin core facade. `conformance/http3_qualification.toml` records deterministic, manual, and blocked evidence separately; upstream correctness risk and missing independent-client/adversarial evidence keep H3 experimental.
- **Downstream substrate closure (Plans 172/177, program closure 208)** — Plans 172–175 close the qualified HTTP-only downstream-substrate line; Plan 199 implements the generic tunnel successor to deferred Plan 176. Plan 205 observability hooks are explicitly deferred (no new observer/event/timing API; Plan 181 `OpsContext` stays the boundary). Keep separate application-server work in its own project and preserve the Plan 175 public-API/bounded-coordination boundary.
- **Ecosystem interop (Plan 200)** — optional `http-interop` (`primitives::interop`: loss-aware `http` conversions, `RequestBody: http_body::Body` with data+trailers, `response_from_http_body` framing-authoritative) and `tower` (`server::tower`: per-request clones driving `poll_ready`, adapter-local ready). Header cross-name order does not round-trip; opaque values use `from_bytes`; interim/tunnel never enter `Extensions`; middleware runs after parsing/validation, before normalization (see `docs/http-interop.md`). Never add Tower/`http` to default builds.
- **Body policy** — The policy is evaluated for the actual method; GET/HEAD/DELETE/OPTIONS/extension bodies are not globally rejected. TRACE content remains rejected. `StaticService` declares `Reject`; bodyless unsupported static methods receive 405, while body-bearing requests may be rejected by policy first.
- **Python RequestBody** — `RequestBody.read()` and `RequestBody.iter_chunks()` are mutually exclusive. `iter_chunks()` bridges async Rust body to synchronous Python via bounded channel with backpressure.
- **Structured logging** — `eggserve-core::ops` provides the event model (`Event`, `EventKind`, `Severity`, `Logger`, `LogSink`, `OpsCounters`, per-runtime `OpsContext`). The CLI initializes with `StderrLogSink` (adopted into the global default). Runtime code uses the explicit context from `RuntimeState`/`ConnectionActivity` (`ops.emit(...)`), never `Logger::global()`; CLI/frontend init keeps the global compat path. `RuntimeState::with_ops` / `ServerBuilder::ops_context` attach explicit contexts (`new`/`try_new` clone the global default); snapshots via `RuntimeState::ops_snapshot()` / `ServerHandle::ops_snapshot()`; connection IDs start at 1 per context. Sink-failure accounting is context-local (`OpsContext::with_sinks` / `CompositeLogSink::with_failure_counters`); plain `new()` keeps global accounting. Containment stays non-recursive: child panics are caught, `dropped_log_events` is the signal, healthy siblings still receive the event, never re-emit through a logger. Library crates must not use `println!`/`eprintln!`. `Logger::try_init()` exists for Python bindings coexisting with CLI init; never call `Logger::init()` twice. `CompositeLogSink` contains child panics, increments `dropped_log_events`, continues with healthy siblings, and never re-emits via `Logger::global()` (no recursive sink-graph traversal; the counter is the signal).
- **Listener error classification** — Accept errors are classified by `io::ErrorKind` into transient/resource-exhaustion/persistent categories with bounded exponential backoff. Use `classify_accept_error()` helper.
- **Listener ownership (Plan 201)** — one `accept_loop_multi` drives TCP + Unix. Builder: `from_std_listener`, `from_unix_listener` / `from_std_unix_listener` (Unix-only, never unlink), `from_systemd_index` / `from_systemd_name` (explicit selection, `SOCK_STREAM` + `SO_ACCEPTCONN` + family via `rustix::net`, failure never closes), `http3_socket(std UdpSocket)` (same-port validation). Handles expose stable `tcp-0`/`unix-0` IDs via `endpoints()`; `local_addr()` panics on Unix-only (use `tcp_local_addr()`); Unix is plaintext (`ConnectionContext::for_unix()`).
- **Trusted proxy and PROXY protocol (Plan 202)** — `RuntimeConfig.trusted_proxy` (explicit peers/CIDRs, no DNS/implicit loopback; Unix explicit `trust_unix`; PROXY disabled 5s timeout, 107B v1 / 16+≤1024B v2, `PrefixedIo` replay, `TCP → PROXY → TLS → HTTP`; disabled interprets bytes normally; malformed/untrusted closes before service; `LOCAL`/`UNKNOWN`/`UNSPEC`/UNIX truthful absence; TLVs ignored bounded). Per-request `Forwarded`/`X-Forwarded-*` single-hop rightmost-wins, conflict fail-closed, `unknown`/obfuscated → `None`, canonical Host/target never rewritten. H1/H2 share pipeline; H3 ignores. Observability: `proxy_protocol_accepted`/`rejected` + `forwarded_metadata_accepted`/`rejected` with sanitized fields/counters. Tower via `ConnectionInfoExt`; Python `lowlevel` exposes `trusted_proxies`/`trust_unix_local`/`proxy_protocol`/`forwarded_*` + `effective_*` getters with `remote_addr` unchanged. Still no reverse proxying.
- **Production TLS identity (Plan 203)** — `tls::TlsServerConfigBuilder`/`TlsServerConfig` (SNI exact + single-level `*.suffix` + optional default via maintained `ResolvesServerCert`, no IO in `resolve`, 64 identities/253-char bound, `keys_match` before ready, never log key bytes) + WebPKI mTLS (`Disabled`/`Optional`/`Required` via `client_auth_*`, 256 roots/16 CRLs/1 MiB bound, no revocation implied without CRLs, no Python handshake callback). `TlsInfo` extends to `alpn`/`client_authenticated`/`peer_certificates_present`/opt-in bounded `peer_certificate_chain` (8×64 KiB via `tls_expose_peer_chain`, default false). Accept order `TCP → PROXY → TLS deadline → ALPN → HTTP` (sanitized errors, permits released once, ALPN from `http2.enabled`); `RuntimeConfig.tls_reload_handle` wins over `tls_config`, `ServerHandle::replace_tls_config` is atomic for new handshakes (failed builds never touch live, no watcher, established keep session); `max_early_data_size=0` + `NeverProducesTickets` explicit; H3 keeps separate TLS 1.3/`h3` QUIC identity (TCP reload does not rotate H3, endpoint replacement/drain required). CLI/Python `HTTPSServer` stay single-identity compatible; advanced TLS is Rust-first (see `docs/tls.md`, `tests/tls_identity.rs`).
- **Foundation maintainability (Plan 206)** — behavior-preserving module boundaries; public import paths preserved via re-exports (`primitives::canonical::X`, `primitives::X`, `server::RuntimeState`, `server::Py*` still resolve); cross-module helpers are `pub(super)` (parent-visible, never widened for convenience). Ownership: `ops/` (`mod` authority + `events`/`sinks`/`counters`); `primitives/canonical/` (`status`/`headers`/`response_body`/`response`/`adapters`; `Response.body` + `remove/strip` are `pub(super)`; tests stay in facade); `server/config/` (`runtime` single validation authority delegating to `runtime_limits` + `http1`/`http2`/`http3`/`tls` protocol owners; `Http2/3::validate` are `pub(super)`; `http1.rs` is a retained inventory placeholder with no H1 authority, Plan 249); `server/http3/` (`endpoint`/`request`/`response`/`tunnel`; `accept_loop` qualifies as `endpoint::`/`request::`/`response::`/`tunnel::`; one shared kernel, no H3-specific semantics); `server/` (`runtime.rs` owns `RuntimeState`, `accept.rs` owns `accept_loop_multi`/handlers/sources/TLS helpers with `pub(super)` enums/fns; facade keeps `Server`/`ServerBuilder` + re-exports); `eggserve-python/src/server/` (`errors`/`body_bridge`/`request_bridge`/`tunnel_bridge`/`response_bridge`/`static_responder`/`sync_handler`/`runtime` + `lifecycle`/`async_handler` pointers; async Plan 204 stays Python-side in `lowlevel.py` with no duplicated Rust conversion; PyO3 registration stays small in facade). Static planner stays pure with the explicit one-way `StaticService::canonical_response()` adapter (no duplicate status/header/body validation). No wire/security/lifecycle behavior change; no line-count gates.
- **Cross-protocol conformance (Plan 207)** — one normative inventory (`conformance/app_server_conformance.toml`: 55 scenarios, 47 routine) drives qualification across H1 TCP/TLS/prebound/Unix, H2 prior/TLS/prebound, H3 QUIC, and caller-owned duplex with native/`http`/Tower/async-Python/ASGI consumers. Routine subset lives in `crates/eggserve-core/tests/cross_protocol_conformance.rs` (H1 + prebound + Unix + caller-owned + H2/Tower-gated); H1 TLS/H2 TLS/H3/`http-interop`/async-Python/ASGI are owned by their existing suites and referenced, not duplicated. Expensive two-client/browser/soak/impairment/perf evidence stays manual and fail-closed (`qualify-http2.sh`/`qualify-http3.sh` + `release/plan-207-cross-protocol-conformance.md`). No tier promotion follows; H2/H3 stay experimental.
- **`ResolvedFile` extraction methods** — `from_parts()`, `into_std_file()`, `into_parts()` are `pub` on the static authority behind the `python-bindings-internal` feature (forwarded by `eggserve-core`'s feature of the same name) for cross-crate Python bindings but carry security caveats: confinement guarantee ends after extraction.
- **Python server façade** — `eggserve.server` is the supported six-class API, including rustls-backed `HTTPSServer` and `ThreadingHTTPSServer` with HTTP/1.1 ALPN only. The exact fast-path eligibility and intentional incompatibility contract is maintained in `docs/python-http-server-compatibility.md`. Stock static handlers also support `default_content_type` and ordered safe `extra_response_headers`; those headers are limited to final 200 responses. Handler `protocol_version` must remain HTTP/1.1. Subprocess helpers are canonically owned by `eggserve.subprocess` (`eggserve.server` keeps compatibility re-exports without expanding `__all__`; top-level `serve_directory` re-exports the subprocess implementation).
- **Python lowlevel substrate (Plan 166)** — `eggserve.lowlevel` exposes handler-only `Server(config, handler)` (no static root, same native runtime, no second accept loop), frozen `RuntimeConfig` (Plan 164 controls + safe privacy subset: `server_header`/`date_policy` system|suppress/`stripped_response_headers`/`error_policy` minimal|empty; Plan 202 trusted-proxy subset: `trusted_proxies`/`trust_unix_local`/`proxy_protocol`/`forwarded_standard`/`forwarded_legacy`, all default nothing trusted; `None` disables, `0` never unlimited; projected via the single `_native_kwargs()` helper, Plan 182), bounded `Response.stream(status, iterable, headers, content_length)` over a 16-chunk bridge (HEAD/body-forbidden never advance the iterator; sync iterables only — async via `AsyncResponse.stream`; non-bytes/iterator errors truncate with sanitized type-only logs; no `Transfer-Encoding` from services), and caller-owned `StaticResponder` composition (no routing in EggServe). `Request` exposes `remote_addr` unchanged plus `effective_addr`/`effective_scheme`/`effective_authority`/`proxy_provenance`/`forwarded_provenance` (absent without explicit trust). Plan 204 adds experimental `AsyncServer(config, async_handler, max_async_tasks)` (H1-only, same runtime, manual asyncio bridge, no new deps; `AsyncRequest` byte-fidelity + `read_chunk`/`trailers`/`send_interim`/`take_tunnel`, `AsyncResponse.stream` over async iterables via bounded 16-queue + `stream_with_trailers`, one-shot `Tunnel` duplex; ASGI fixture in `crates/eggserve-python/tests/asgi_fixture.py` only, not a product).
- **CLI compatibility polish** — Manual parsing accepts hostname `--bind` values, repeatable `-H/--header` and `--content-type` static metadata, and a combined certificate/key PEM when `--tls-key` is omitted. Header metadata is validated against runtime-owned and hop-by-hop fields. Production admission/lifecycle CLI flags: `--max-in-flight-requests`, `--keep-alive-idle-timeout`, `--max-requests-per-connection` (`0` = unlimited), `--response-write-timeout`, `--max-buf-size`, `--max-headers`, `--max-header-bytes`, `--max-request-target-bytes`.
- **Python wheel support** — CPython 3.11+ with abi3 stable ABI. Routine CI builds and tests the Linux wheel; macOS and Windows wheels are built manually. Release wheels target 9 platforms: manylinux_2_17 (x86_64, aarch64, armv7l), musllinux_1_2 (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64, arm64).
- **Semaphore bounds** — `max_connections`, `max_file_streams`, and `max_in_flight_requests` are validated once in the Plan 179 kernel (`crate::runtime_limits`) against `tokio::sync::Semaphore::MAX_PERMITS`. Values above this bound are rejected with a controlled error.
- **Plan 164 admission/lifecycle fields** — `RuntimeConfig`/`Limits` own `max_buf_size` (65536, Hyper min 8192), `max_headers` (100, pinned explicitly; Hyper answers excess with 431), `max_header_bytes` (32 KiB, 431 pre-service), `max_request_target_bytes` (8192, 414 pre-service), `max_in_flight_requests` (64, 503 on exhaustion, held across `Service::call`), `keep_alive_idle_timeout` (60s, resets on activity), `max_requests_per_connection` (`Option<u64>`, `None` = unlimited; CLI `0` = unlimited), `response_write_timeout` (30s, no-progress via `ProgressIo` + `TrackedBody`). Idle/write timeouts are NOT cross-checked against `connection_total_timeout`. `ConnectionOutcome` adds `IdleTimeout` (clean) and `WriteTimeout`. Hyper is 1.11.1: lone TE+CL normalizes to TE-wins (200), only duplicate/conflicting CLs still fail; Hyper also applies `header_read_timeout` while keep-alive idle, so set idle shorter for distinct accounting or raise both for long-lived keep-alive. Per-profile defaults live in `docs/deployment.md`; full semantics in `docs/timeout-reference.md`.
- **Plan 179 canonical runtime authority** — shared runtime defaults/validation live once in `eggserve-server::runtime_limits` (`SharedRuntimeValues` + `Violation`, public; `eggserve_core::runtime_limits` re-exports with `From` adapters next to `Limits`/`RuntimeConfig`). `Limits::validate()` delegates shared checks + static listing budgets; `RuntimeConfigBuilder::build()` validates the shared group + `ResponsePolicy`; `try_from_serve_config()` projects via `RuntimeConfig::from_shared_runtime`. Static listing/extra-header budgets stay service-owned; frontend-only controls stay in their surfaces. Services may lower `max_request_body_bytes` but never raise the hard ceiling.
- **Logging modes** — `--log-format none` uses `NopLogSink` (no output). `--quiet` wraps the format-specific sink with `FilteredLogSink` (warn/error only). Direct argument-validation errors printed before logger initialization may remain on stderr.
- **Release validation** — run `bash scripts/install-cargo-tools.sh` followed by `bash scripts/check-supply-chain.sh`; this covers both distributed lockfiles. Release builds use exact Rust 1.98.1 while compatibility lanes retain floating stable.
- **Unsafe Rust policy** — workspace `unsafe_code = "deny"` is inherited by the workspace crates and the excluded Python manifest declares the same lint locally. Only the reviewed FFI/test boundaries in `docs/unsafe-code-policy.md` carry local exceptions.
- **`server` module is experimental** — `eggserve-core::server` provides the runtime service boundary. Its API is subject to change without notice.
- **Production profiles** — Production profiles are documented in README.md and `docs/deployment.md`. Every production claim must name a profile. Hardened profiles must not allow symlink following. Windows is functionally qualified, but remains trusted/local-content only because two open-descendant root-rename cases are rejected by NTFS path-rename semantics; see `docs/toolchain-support.md`.
- **`ops` module** — `Logger` uses `OnceLock` for global initialization. `try_init()` is for Python bindings that may coexist with CLI initialization (and adopts the sink into the global `OpsContext` default). Do not call `Logger::init()` twice. Runtime code uses the explicit `OpsContext`, not the global.
- **No println/eprintln in library code** — Runtime code must use the explicit `OpsContext` (`ops.emit(...)`); CLI/frontend init keeps the `Logger::global()` compat path.
- **Examples are product demonstrations** — Use the canonical examples in `examples/README.md` when documenting CLI, Python, or Rust usage. Rust examples must use public APIs only; listener-based server examples bind loopback, support port `0` for smoke tests, wait for readiness, and cleanly shut down on Ctrl+C (`caller_owned_stream.rs` binds nothing by design; `primitives.rs` opens no socket). Python examples expose `create_server()` for smoke tests. Do not turn examples into a framework, router, or alternate policy reference.
- **Qualification evidence (Plans 168/170/227–241)** — These plans are qualification phases: deterministic suites per track plus manual, same-machine performance evidence (see `architecture/testing-and-conformance.md`), never absolute-timing CI gates. Plan 229/232's current file-stream default is 128 KiB, retained by the live 64/128 KiB throughput/resource comparison and bounded by `max_file_streams * stream_chunk_size`; `read_file_chunk` uses an explicit logical target and never allocator capacity. Plans 234–241 retain canonical request-target/header/body representations, hardened root-FD/path fast paths, generic H1 dispatch, zero-tunnel checks, and lazy Python compatibility views without changing public behavior; Plan 238 is NO-GO and the Python producer-thread redesign is DEFER. Direct H1 write timeout progress is actual socket-write progress, while H2/H3 retain protocol-specific producer semantics. Benchmark methodology, regression policy, and claims policy live in `benchmarks/README.md`; machine-readable results are in `benchmarks/088-baseline/results.json`, `benchmarks/168-qualification/results.json`, `benchmarks/170-closure/results.json`, `benchmarks/227-current-head/`, `benchmarks/231-optimization-closure/`, `benchmarks/232-corrective/`, `benchmarks/233-evidence-polish/`, `benchmarks/234-fixed-cost-baseline/`, `benchmarks/240-fixed-cost-closure/`, and `benchmarks/241-fixed-cost-evidence-corrective/`. Manual performance qualification must retain compact per-trial JSON rather than only aggregate reductions of discarded captures. Every performance/release claim must name a profile + workload + evidence; forbidden claims (edge-proxy parity, DDoS resistance, un-fingerprintability, ASGI/WSGI parity, HTTP/2/3, universal superiority headlines) stay forbidden.
- **Plan 241 evidence corrective** — `benchmarks/241-fixed-cost-evidence-corrective/` closes the missing custom H1, static response-shape, established-TLS, installed-wheel Python view, slow-stream resource, and Unix resolver syscall evidence with compact per-trial records and explicit CONFIRMS/NEUTRAL/N/A classifications. It is evidence-only: no production changes, timing gates, metadata-sharing reopen, Plan 238 shared-state change, or Plan 239 producer redesign. Unavailable peer-chain exposure remains explicitly unavailable.
- **Response-planning edge semantics** — Inverted ranges (`start > end`, e.g. `bytes=50-10`) are invalid specifiers and the Range header is ignored (full 200), never 416 (RFC 9110 § 14.1.2); 416 is only for unsatisfiable-but-valid ranges (start beyond EOF, empty file). `evaluate_if_match("*", None)` is `false` (no representation, nothing to match). HEAD normalization retains a known representation length via `ResponseBody::EmptyWithLength` (zero wire bytes; unknown lengths stay `Empty`); body-forbidden statuses still normalize to `Empty`. A literal `#` with no `?` is an ordinary path character (documented, tested), not a fragment delimiter.
- **Rust package boundary** — `eggserve-core` is the intended 0.x Rust library crate. `eggserve-core::primitives` is the semver-considered facade; `eggserve-core::server` is experimental. `eggserve-bin::run_cli` exists for the Python wheel's extension-backed CLI and is not a general Rust embedding API. Do not add a facade crate for naming convenience.
- **Rust closure verification** — For library/CLI usability work, run `cargo test --doc -p eggserve-core`, `cargo check -p eggserve-core --examples`, both dist builds, and `bash scripts/verify-cargo-packages.sh --mode all`; use a temporary clean external consumer for static and custom-service TCP smokes when the plan requires it. For native bind/TLS changes, run the manual platform qualification workflow after pushing the final SHA.
