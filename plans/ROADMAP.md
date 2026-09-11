# eggserve roadmap

## Purpose

eggserve is a hardened, auditable, Rust-backed replacement for the common `python -m http.server` use case and a reusable set of safe HTTP/static-serving primitives. Static serving remains the primary end-user product. EggServe is not itself an application server, ASGI/WSGI runtime, reverse proxy, framework, CDN, or Granian-style general server; its Rust core also exposes a hardened, transport-owning HTTP runtime and canonical service boundary that separate downstream application-server projects may embed. Its core value is a small, predictable, security-oriented substrate that gives Python users standard-library-like ergonomics with production-grade defaults.

The initial public surface should look familiar:

```bash
python -m eggserve
python -m eggserve 8000
python -m eggserve --directory public
python -m eggserve --bind 127.0.0.1 --port 8000
python -m eggserve --directory public --public
```

The long-term public surface should also expose conservative Python primitives:

```python
from eggserve import serve_directory, ServeConfig, StaticPolicy

serve_directory(
    "public",
    bind="127.0.0.1",
    port=8000,
    policy=StaticPolicy.safe_default(),
)
```

The Python compatibility API should remain narrow. Framework and application
semantics—routing, middleware ecosystems, templating, sessions, reverse
proxying, and application lifecycle—remain outside EggServe. The Rust core may
provide generic HTTP request/response streaming, lifecycle and cancellation
primitives, and service embedding; those capabilities are substrate support,
not an application-server implementation. A separate downstream project owns
ASGI/WSGI event models, Python event-loop integration, worker/process
management, framework loading, lifespan, and application concurrency policy.

## Product principles

1. Safety over exact `http.server` compatibility. Compatibility should be ergonomic and operational, not behavioral. Unsafe standard-library behaviors must not be preserved by default.
2. Explicit policy. Filesystem, path, symlink, dotfile, directory listing, MIME, caching, logging, and bind-address behavior should be visible and configurable through typed policy structures.
3. Controlled protocol scope. HTTP/1.1 remains the minimal/default compatibility baseline. Optional HTTP/2 and HTTP/3 runtime support is governed by Plans 183–190 for scope, implementation, deterministic qualification, and corrective closure; Plans 191–193 may promote those existing transports only through explicit independent-client, adversarial, dependency, platform, and release-evidence gates. None of those plans authorizes unrelated edge-server or framework features. Static/default services reject request bodies by default, while custom Rust services may opt into bounded request-body streaming through the experimental runtime seam.
4. Small dependency graph. Hyper is the HTTP/1/2 substrate. Avoid `reqwest`, full web frameworks, reverse-proxy stacks, templating engines, and broad middleware systems unless a specific milestone justifies them. HTTP/3/QUIC dependencies remain optional and isolated from the minimal build.
5. Auditable implementation. Security-critical behavior should live in small, independently tested modules with fuzz targets and regression corpora.
6. Stable foundation before features. Range requests, TLS, CORS, custom directory rendering, Python APIs, Rust library stabilization, and additional protocols should follow only after the path confinement and resource-limit model is proven.

## Architectural target

The repo should converge on a workspace similar to:

```text
crates/
  eggserve-core/       # policy, path confinement, static serving, canonical HTTP, runtime/service boundary
  eggserve-bin/        # Rust CLI binary
  eggserve-python/     # Python wheel packaging and python -m launcher
fuzz/
  fuzz_targets/
    path_target.rs
    percent_decode.rs
    request_target.rs
plans/
docs/
tests/
```

## Current downstream-substrate position

Plans 161 and 172–175 explicitly extended the reusable Rust boundary after the
original static-serving milestones. The qualified capability is a hardened
HTTP transport/runtime substrate plus the experimental generic tunnel handoff:
separate projects may build application servers against the public canonical
primitives and experimental `server` APIs, with bounded downstream coordination
and application-task admission owned there. Plan 199 implements the generic
tunnel successor to deferred Plan 176 (one-shot `TunnelCapability` +
bounded `TunnelIo`; denial stays ordinary HTTP; WebSocket framing stays
downstream).
This does not make EggServe an application server or promote experimental
runtime types to the stable 1.0 API. Plan 205 (application observability
hooks) is explicitly deferred by Plan 208: the Plan 181 per-runtime
`OpsContext` remains the observability boundary, and no `RequestObserver` /
request-ID / lifecycle-event / timing extension is promised.

The core crate should have no Python awareness. The binary should be a thin consumer of the core crate. The Python package should initially be a very thin launcher for the Rust binary, not a premature extension API. Once the core is stable, expose a Python API as a narrow wrapper around typed Rust configuration.

## Protocol expansion, corrective closure, and support promotion — Plans 183–194

Plan 183's product/scope gate has been implemented and the live product contract in `docs/non-goals.md` now authorizes only the narrow native H2/H3 transport work described by this program. Plans 184–188 implemented and qualified the first protocol adapters, leaving H2 and H3 experimental. Plans 189–190 closed deterministic semantic gaps discovered by the post-188 review without changing those support tiers. Plans 191–193 are evidence-led promotion gates: they may promote the already-implemented protocol transports, but they do not add another protocol family or broaden the product surface. Plan 194 is a narrow H3 producer-timeout + promotion-trace correction with no promotion authority.

```text
183  HTTP/2 and HTTP/3 protocol expansion roadmap / product gate
 |
184  Protocol-neutral runtime preparation and overlap cleanup
 |
185  HTTP/2 runtime, TLS/ALPN, multiplexing, and hardened limits
 |
186  HTTP/2 conformance, interoperability, and release closure
 |
187  HTTP/3 QUIC transport and canonical adapter
 |
188  HTTP/3 interoperability and multi-protocol release closure
 |
189  Multiprotocol request-body, error, and lifecycle correctness
 |
190  Multiprotocol corrective qualification and release closure
 |
191  HTTP/2 supported-tier promotion qualification

  192  HTTP/3 dependency readiness and conformance hardening
  |
  193  HTTP/3 supported-tier promotion qualification
  |
  194  HTTP/3 response-producer timeout and promotion-trace correction
```

Plan 183 updates the product/non-goal and pre-1.0 API contract. Plan 184 removes the remaining duplicated request-target parser, repeated service-invocation logic, HTTP/1-specific lifecycle decisions in shared code, lossy version conversion, latent upgradeable-connection machinery, and ambiguous protocol-specific configuration ownership while proving HTTP/1 behavior unchanged.

Plan 185 adds HTTP/2 through the existing Hyper/Hyper-Util family, with explicit H2 stream/header/flow-control limits, TLS ALPN, stream-scoped body/error handling, stream-aware response activity, and GOAWAY/drain semantics. Plan 186 provides the initial independent-client, adversarial/resource, shutdown, platform, footprint, and documentation evidence; its executed result keeps H2 experimental because meaningful second-implementation/browser/platform evidence and per-stream reset/progress limitations remain.

Plan 187 treats HTTP/3 correctly as a separate QUIC/UDP transport implementation sharing the same canonical service layer. Its H3/Quinn dependencies remain optional/internal; it owns dual-listener lifecycle, TLS 1.3/`h3` ALPN, handshake/stream/QPACK budgets, canonical H3 adaptation, stream-specific backpressure/cancellation, GOAWAY/drain, and runtime-owned Alt-Svc. Plan 188 closed the feature at the experimental tier after deterministic checks; external H3 interoperability, network-impairment/resource evidence, and cross-platform runtime qualification remained explicit follow-up gates.

Plan 189 corrects the narrow post-188 findings: H2/H3 Reject-body handling detects DATA without relying on `Content-Length`, H3 runtime errors share the canonical representation authority, H3 provides connection/stream `RequestLifecycle` cancellation parity, and H2 response-progress wording/behavior matches what the public Hyper stack can actually observe. Plan 190 directly reproduces those bug classes, re-runs available protocol qualification, synchronizes plan/release documentation, and closes the corrective pass while retaining H2/H3 as experimental.

Plan 191 is a qualification-led HTTP/2 promotion attempt. It requires at least two independent H2 implementation families rather than two libnghttp2 frontends, at least one current browser, current-RFC conformance classification, multiplexing/reset/header/flow-control/GOAWAY/resource tests, and real Linux/macOS/Windows runtime evidence. H2 may become **supported, opt-in** without becoming default-enabled and without falsely promising a Hyper stream-local wire-progress/reset capability.

Plan 192 was the mandatory HTTP/3 dependency-readiness gate before promotion, executed 2026-09-10 with a `BLOCKED` outcome. It froze the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11 — no upgrade candidate), found `hyperium/h3#338` open with no released fix, fixed three `hyperium/h3#262` early-error paths while recording three residual ones, and left H3 experimental with concrete blockers (see `release/plan-192-http3-dependency-readiness.md`). Plan 193 closed at preflight the same day without entering promotion qualification: the Plan 192 prerequisite was still `BLOCKED`, so the pass inventoried the unchanged candidate, re-checked `#338`/`#262` as still open, and recorded two-family interop, browser Alt-Svc, adversarial-frame, network-impairment, and cross-platform H3 runtime evidence as unavailable (see `release/plan-193-http3-supported-tier-qualification.md`). Plan 194 (same day) bounds the H3 `ResponseStream` producer poll with an absolute `response_write_timeout` no-progress deadline (empty chunks are not progress) plus `WriteStallTimeout` observability and corrects the H2-vs-H3 timeout wording across the live docs, without changing the experimental tier or the Plan 192/193 blockers (see `release/plan-194-http3-producer-timeout-correction.md`). Plan 195 (2026-09-11) correctively qualifies that bound with reproducible evidence — stalled, progress-then-stall, slow-progress, empty-chunk, and sibling isolation plus new shutdown-race drain and write-stall observability/permit-release regressions (H3 suite 14 → 16) — with no source change and no tier change (see `release/plan-195-http3-response-timeout-corrective-qualification.md`). A future H3 promotion requires a new scoped plan closing those blockers; Plans 193–195 are no longer open promotion authorities.

The program explicitly does **not** authorize WebSockets, WebTransport, datagrams, CONNECT tunnels, server push, reverse proxying, ACME, DNS HTTPS/SVCB automation, routing, middleware, uploads, application workers, or in-tree ASGI/WSGI semantics. The six-class Python `http.server` compatibility facade remains HTTP/1.1-shaped unless a later separate product decision changes it.

## Default security posture

The safe default should be deliberately conservative:

```text
bind address: 127.0.0.1
methods: GET, HEAD
request bodies: rejected
HTTP version: HTTP/1.1 compatibility baseline; H2/H3 remain opt-in and may become supported only through Plans 191–193 without becoming default-enabled
directory listing: disabled unless explicitly enabled
index files: enabled for index.html by default
symlinks: denied by default
dotfiles: denied by default
unknown MIME: application/octet-stream
public bind: requires explicit opt-in or loud warning
logging: sanitized text logs by default
TLS: optional feature, not required for minimal build
```

Path handling is the critical security boundary. eggserve must not rely on a naive `canonicalize(root.join(path)).starts_with(root)` model as the final design. The path layer should be treated as an independently auditable subsystem with platform-specific behavior. Unix should move toward descriptor-relative traversal where practical. Windows should explicitly handle drive prefixes, UNC-like paths, reserved names, alternate data streams, reparse points, and separator ambiguity.

## Milestones

### M0: repository foundation and security contract

Create the repo skeleton, threat model, non-goals, dependency policy, initial architecture notes, and release criteria. This milestone establishes what eggserve is and is not. It should produce documentation that future contributors can use to reject scope creep.

Exit criteria: the repo contains docs for threat model, security policy, non-goals, dependency policy, initial architecture, and compatibility boundaries. CI can run formatting and basic checks even before full implementation.

### M1: Rust core skeleton and HTTP substrate

Create the Cargo workspace and initial crates. Add the Hyper/Tokio HTTP/1.1 accept loop, service entry point, typed configuration, error taxonomy, and basic `GET`/`HEAD` placeholders. No serious static serving should ship before the policy modules exist.

Exit criteria: `cargo test` and `cargo check --workspace` pass; a minimal server can return a static placeholder response; unsupported methods return deterministic errors; connection limits and graceful shutdown have initial scaffolding.

### M2: path confinement and filesystem policy

Implement the security-critical path pipeline: request-target handling, percent decoding, component validation, dotfile policy, symlink policy, root confinement, and platform-specific denial cases. Add unit tests, fixture tests, and fuzz targets.

Exit criteria: no accepted path can escape the configured root under the safe default policy; traversal, double-encoding, absolute-path, Windows-prefix, NUL, dotfile, and symlink regression tests exist; the path module is independently testable without starting the server.

### M3: static file serving MVP

Serve regular files using `GET` and `HEAD` with correct `Content-Length`, conservative `Content-Type`, `Last-Modified`, optional ETag support, index handling, and directory denial/listing behavior. Do not add Range or compression yet.

Exit criteria: a real directory can be served safely; `HEAD` mirrors `GET` headers without a body; directories without an index are denied unless listing is explicitly enabled; generated listing output is HTML-escaped and protected by conservative headers.

### M4: resource limits and operational hardening

Add header/request-target limits, connection concurrency limits, file-serving permits, read/write/idle timeouts, slow-client resistance, sanitized logging, and graceful shutdown behavior. Establish load and adversarial behavior tests.

Exit criteria: slowloris-style clients cannot hold resources indefinitely; high concurrency fails predictably; logs cannot be trivially injection-poisoned; large-file serving is bounded by explicit permits; all defaults are documented.

### M5: CLI parity and Python wheel launcher

Implement `eggserve` CLI and `python -m eggserve` packaging. Keep the Python layer thin at first. Provide the familiar `http.server`-like workflow while making unsafe behavior explicit.

Exit criteria: wheels build for the first supported platforms; `python -m eggserve --directory public 8000` works; CLI prints effective policy; public bind and unsafe flags are visible; package metadata and README accurately describe scope.

### M6: fuzzing, CI matrix, and security validation

Expand fuzz targets, add cargo-audit/cargo-deny/cargo-vet where appropriate, run platform CI, and add regression fixtures for path and HTTP behavior.

Exit criteria: Linux, macOS, and Windows checks pass; fuzz targets are documented; dependency policy is enforced; security regression tests are part of normal CI.

### M7: optional TLS and deployment guidance

Add optional `rustls` support under a feature flag. Document native TLS and reverse-proxy deployment patterns. Do not implement ACME in eggserve.

Exit criteria: TLS cert/key serving works when the feature is enabled; minimal builds do not pull TLS dependencies; deployment docs explain Caddy/nginx/Traefik/load-balancer fronting.

### M8: minimal Python API

Expose stable Python functions and configuration classes after the core behavior is proven. Keep the API synchronous and static-serving-oriented.

Exit criteria: Python users can call `serve_directory(...)` and configure safe policies without interacting with Rust details; API docs clearly state non-goals; no dynamic request callback API is introduced.

### M9: library stabilization and 1.0 preparation

Stabilize Rust primitives, document compatibility guarantees, finalize default policies, run a security review, and prepare crates.io/PyPI release workflows.

Exit criteria: public APIs are documented; unsafe choices are opt-in; release checklist is repeatable; project has a clear 1.0 security posture.

## Initial dependency policy

The initial dependency set should be small and justified:

```text
tokio: async runtime
hyper: HTTP/1/2 protocol substrate
hyper-util: Hyper 1.x server/runtime utilities
http-body-util: response body helpers
bytes: efficient byte buffers
percent-encoding or equivalent: path decoding, if selected after review
pico-args or minimal parser: CLI argument handling
tracing/tracing-subscriber: optional structured logging
rustls/tokio-rustls: optional TLS feature only
QUIC/H3 stack: optional only under Plans 187–194; never required by the minimal build
```

Avoid `reqwest`, Axum, Tower, Tera, Askama, libmagic bindings, compression stacks, ACME clients, database crates, and app-framework dependencies in the initial milestones.

## Release gates

An alpha can ship after M0-M5 if the docs clearly mark it as early and the unsafe areas are not exposed. A beta should require M6. A production-ready 1.0 should require M7-M9, a platform test matrix, dependency audit, fuzz corpus, and a written security review.

Optional HTTP/2/HTTP/3 support does not become part of the release promise merely because code exists. Plans 186/188 and corrective Plans 189–190 leave both transports experimental after deterministic qualification. Plan 191 executed the H2 promotion attempt and retained the experimental tier: two-family interop, h2spec classification, and flow-control/load evidence were collected, but browser evidence, macOS/Windows runtime evidence, trailer-scope determinism, and the stream-local reset hook remain open (see `release/plan-191-http2-supported-tier-qualification.md`). A future H2 promotion requires a new scoped plan closing those blockers; Plan 191 is no longer an open promotion authority. H3 additionally closed Plan 192 dependency readiness as `BLOCKED` (latest released `h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn 0.11.11; upstream `h3#338` unfixed, `#262` remainder open; see `release/plan-192-http3-dependency-readiness.md`), Plan 193 closed at preflight on 2026-09-10 without entering promotion qualification (unmet Plan 192 prerequisite; unchanged candidate; `#338`/`#262` still open; two-family, browser, adversarial, impairment, and platform evidence inventoried as unavailable; see `release/plan-193-http3-supported-tier-qualification.md`), and Plan 194 bounds the H3 producer poll with an absolute no-progress deadline (empty chunks are not progress) without changing the tier (see `release/plan-194-http3-producer-timeout-correction.md`), and Plan 195 correctively qualifies that bound (shutdown-race and observability regressions, H3 suite 14 → 16) without changing the tier (see `release/plan-195-http3-response-timeout-corrective-qualification.md`). A future H3 promotion requires a new scoped plan closing those blockers; Plans 193–195 are no longer open promotion authorities. Either protocol may remain experimental independently while HTTP/1/static serving continues to ship.

Support-tier promotion never implies default enablement. The minimal/default product remains HTTP/1.1-shaped, and the Python compatibility facade remains HTTP/1.1-shaped unless a separate future product plan explicitly changes it.

The current stable-Rust API line also contains documented pre-1.0 breaking changes and must not be published as a `0.1.x` patch release. Release preparation should use the synchronized metadata ownership established by Plan 182 and publish that line as `0.2.0` or later, with the migration guide/release notes updated in the same release change.

The 1.0 promise should remain bounded: static serving is the primary product;
stable hardened HTTP primitives and policies are the core library promise; and
the transport-owning service/runtime seam is a documented downstream embedding
path according to its stability classification. EggServe does not promise to
be an ASGI/WSGI server, framework, process manager, reverse proxy, or WebSocket
implementation.