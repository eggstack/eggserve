# EggServe Release Readiness Roadmap

## Status

The HTTP primitive roadmap through phase 30 is substantially complete. EggServe now has a hardened static server, a Rust primitive layer, resolver-opened body capabilities, Python server callbacks with Rust-owned I/O, and an experimental low-level HTTP client with optional TLS. The remaining work is release hardening, not broad feature expansion.

## Release objective

Ship a defensible public release of EggServe as:

- a hardened HTTP/1.1 static file server;
- a Rust library exposing auditable request, path, filesystem, response-planning, and body primitives;
- a Python package exposing safe primitives plus a Rust-owned callback server and low-level client;
- a substrate on which downstream projects may build ASGI, WSGI, framework, or client abstractions.

EggServe itself must remain out of scope for ASGI, WSGI, routing, middleware, templating, sessions, reverse-proxy behavior, redirects, cookies, retries, HTTP/2, HTTP/3, WebSockets, and application-framework features.

## Release principles

1. Correct APIs before freezing them.
2. Preserve filesystem and HTTP invariants across every public entry point.
3. Treat packaging and installed-artifact validation as correctness work.
4. Prefer explicit limitations over unsupported security claims.
5. Require evidence for concurrency, timeout, and platform guarantees.

## Phase sequence

### Phase 31 — Release contract and API inventory

Define the exact release surface. Inventory and classify every public Rust and Python API as stable, experimental, internal, deprecated, or removal candidate. Resolve duplicate-header representation, callback response rules, client stability, exception taxonomy, and internal feature exposure before compatibility promises are made.

### Phase 32 — Deterministic concurrency and timeout verification

Replace timing-oriented runtime smoke tests with barrier/counter-based proofs for connection limits, callback limits, file-stream limits, timeout coverage, permit release, disconnect cleanup, and bounded shutdown.

### Phase 33 — HTTP wire-correctness closure

Exercise raw HTTP/1.1 behavior at the socket boundary: request-target forms, malformed headers, framing ambiguity, smuggling-relevant cases, HEAD/204/304 rules, range semantics, response header validation, content-length consistency, and connection lifecycle.

### Phase 34 — Filesystem security closure

Audit every resolution and serving path for race resistance, non-regular files, child-component validation, symlink policy, capability preservation, disappearing/truncated files, and platform-specific behavior. Make an explicit Windows hardening/support decision.

### Phase 35 — Fuzzing and property testing

Expand fuzz coverage across request targets, percent decoding, path validation, range/conditional parsing, response planning, URL parsing, and header validation. Add invariant properties and corpus regression jobs.

### Phase 36 — Client hardening and interoperability

Validate HTTP and HTTPS behavior, certificate verification, timeout coverage, malformed responses, IPv6, body limits, connection-close/chunked bodies, and the deliberately narrow no-pooling/no-redirect/no-cookie client contract.

### Phase 37 — Python boundary hardening

Ensure Python callbacks cannot create malformed responses or violate status/header/body rules. Review GIL and mutex boundaries, callback lifecycle, file-backed response preservation, exception mapping, interpreter shutdown, and body-source reuse.

### Phase 38 — Packaging and installation closure

Test wheels, binaries, and crates in clean environments outside the source checkout with `PYTHONPATH` unset. Verify public imports, native loading, CLI discovery, callback server behavior, client behavior, metadata, licenses, and supported Python/Rust versions.

### Phase 39 — CI, supply-chain, and reproducibility hardening

Complete the platform and feature matrix, minimize workflow permissions, pin release tooling, enforce dependency/license policy, verify archives and checksums, and make release validation reproducible.

### Phase 40 — Performance and resource baselines

Record reproducible throughput, latency, memory, descriptor, task/thread, and shutdown baselines for CLI static serving, Python static mode, callback mode, ranges, TLS, and client requests. Add leak/regression checks.

### Phase 41 — Documentation and examples freeze

Freeze installation, API, security, platform, compatibility, limitation, and release documentation. Ensure every documented example is executable and preserves the capability model.

### Phase 42 — Security review and release audit

Perform a structured review covering request ambiguity, header injection, path confinement, races, file capabilities, range arithmetic, exhaustion controls, GIL behavior, TLS verification, unsafe APIs, and platform gaps. No unresolved high-severity findings may remain.

### Phase 43 — Release candidate process

Build and validate the exact intended artifacts under an RC tag. Run full CI, packaging, fuzz regression, stress, leak, and manual platform smoke tests. Keep the RC feature-frozen.

### Phase 44 — Public release and post-release monitoring

Publish crates, wheels, binaries, checksums, release notes, compatibility/security policies, and known limitations. Establish patch-release and incident-response procedures.

## Release gates

### API gate

- Every public Rust and Python symbol is classified.
- Internal bridge APIs are unavailable under normal features.
- Stable and experimental surfaces are explicit.

### Security gate

- No unresolved critical or high-severity findings.
- Request ambiguity and filesystem confinement have direct tests.
- TLS verification is secure by default.
- Runtime limits are deterministically verified.

### Packaging gate

- Installed-wheel tests run outside the repository and without source-path imports.
- All declared feature combinations compile and test.
- Binary installation and discovery are verified.

### Platform gate

- Supported platforms are exercised in CI.
- Windows is either hardened or explicitly restricted.
- Documentation does not overstate platform guarantees.

### Reliability gate

- Stress and resource-leak tests pass.
- Shutdown is bounded.
- Timeouts cover the operations claimed in documentation.
- Permits are released on success, failure, timeout, and disconnect paths.

### Documentation gate

- Examples match the released APIs.
- Non-goals and limitations are explicit.
- Security, compatibility, and reporting policies are published.

## Recommended execution order

Phases 31 through 35 should be executed next and in order. Phase 31 must precede API freeze. Phases 32–35 establish the evidence required before client/Python/package stabilization in phases 36–39. Phases 40–44 should begin only after the public surface and security invariants are stable.

## Completion definition

The roadmap is complete when EggServe can make the following evidence-backed claim:

> EggServe is a hardened HTTP/1.1 static server and low-level HTTP primitive library with Rust-owned networking and filesystem enforcement, Python bindings for safe server/client construction, explicit platform guarantees, bounded concurrency, verified packaging, and a documented security model.
