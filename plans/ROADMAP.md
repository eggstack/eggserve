# eggserve roadmap

## Purpose

eggserve is a hardened, auditable, Rust-backed replacement for the common `python -m http.server` use case and, more importantly, a reusable set of safe HTTP/static-serving primitives for Python projects. The project is not intended to become an application server, ASGI/WSGI runtime, reverse proxy, framework, CDN, or Granian-style general server. Its core value is a small, predictable, security-oriented substrate that gives Python users standard-library-like ergonomics with production-grade defaults.

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

This Python API should remain narrow. If a feature starts to require routing, dynamic handlers, middleware ecosystems, request body parsing, templating, user sessions, reverse proxying, or app-framework semantics, it belongs out of scope unless a later roadmap explicitly revisits the boundary.

## Product principles

1. Safety over exact `http.server` compatibility. Compatibility should be ergonomic and operational, not behavioral. Unsafe standard-library behaviors must not be preserved by default.
2. Explicit policy. Filesystem, path, symlink, dotfile, directory listing, MIME, caching, logging, and bind-address behavior should be visible and configurable through typed policy structures.
3. Minimal protocol surface. Begin with HTTP/1.1, `GET`, and `HEAD`. Reject unsupported methods. Reject request bodies by default.
4. Small dependency graph. Hyper is the HTTP substrate. Avoid `reqwest`, full web frameworks, reverse-proxy stacks, templating engines, and broad middleware systems unless a specific milestone justifies them.
5. Auditable implementation. Security-critical behavior should live in small, independently tested modules with fuzz targets and regression corpora.
6. Stable foundation before features. Range requests, TLS, CORS, custom directory rendering, Python APIs, and Rust library stabilization should follow only after the path confinement and resource-limit model is proven.

## Architectural target

The repo should converge on a workspace similar to:

```text
crates/
  eggserve-core/       # policy types, path confinement, static serving, response construction
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

The core crate should have no Python awareness. The binary should be a thin consumer of the core crate. The Python package should initially be a very thin launcher for the Rust binary, not a premature extension API. Once the core is stable, expose a Python API as a narrow wrapper around typed Rust configuration.

## Default security posture

The safe default should be deliberately conservative:

```text
bind address: 127.0.0.1
methods: GET, HEAD
request bodies: rejected
HTTP version: HTTP/1.1 initially
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
hyper: HTTP protocol substrate
hyper-util: Hyper 1.x server/runtime utilities
http-body-util: response body helpers
bytes: efficient byte buffers
percent-encoding or equivalent: path decoding, if selected after review
pico-args or minimal parser: CLI argument handling
tracing/tracing-subscriber: optional structured logging
rustls/tokio-rustls: optional TLS feature only
```

Avoid `reqwest`, Axum, Tower, Tera, Askama, libmagic bindings, compression stacks, ACME clients, database crates, and app-framework dependencies in the initial milestones.

## Release gates

An alpha can ship after M0-M5 if the docs clearly mark it as early and the unsafe areas are not exposed. A beta should require M6. A production-ready 1.0 should require M7-M9, a platform test matrix, dependency audit, fuzz corpus, and a written security review.

The 1.0 promise should be narrow: eggserve safely serves static content and exposes hardened primitives for that job. It should not promise to replace a general-purpose web server, reverse proxy, ASGI/WSGI server, or application framework.
