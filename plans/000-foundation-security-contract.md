# Plan 000: repository foundation and security contract

## Goal

Establish eggserve as a security-oriented, auditable, Rust-backed foundation for Python-style static HTTP serving. This pass should create the project contract before implementation choices harden accidentally. The deliverable is not a working server; it is the documentation, repository skeleton, CI baseline, and decision record structure needed to keep the implementation narrow and reviewable.

## Background

The project exists because Python has rich web frameworks and mature application servers, but lacks a standard-library-shaped, hardened, high-performance serving primitive comparable to what users often want when they reach for `python -m http.server`. eggserve should preserve that ergonomic entry point while rejecting unsafe or ambiguous legacy behavior by default.

The core premise is deliberately constrained: serve static content and expose reusable safe primitives. Do not build an ASGI server, WSGI server, reverse proxy, web framework, template engine, plugin host, or dynamic request execution environment.

## Repository skeleton

Create the initial structure:

```text
README.md
LICENSE
SECURITY.md
CONTRIBUTING.md
Cargo.toml
crates/
  eggserve-core/
  eggserve-bin/
  eggserve-python/
docs/
  threat-model.md
  security-policy.md
  non-goals.md
  architecture.md
  compatibility.md
  dependency-policy.md
  release-criteria.md
plans/
  ROADMAP.md
  000-foundation-security-contract.md
```

The crates can be empty or minimal during this milestone, but the workspace should exist so future plans have a concrete target.

## Documentation deliverables

### README.md

The README should state the project in one sentence:

> eggserve is a hardened, Rust-backed replacement for the common `python -m http.server` static-serving workflow and a small foundation library for safe HTTP/static-serving primitives.

It should include a short non-goal statement near the top. Avoid language that implies eggserve is a general web server, framework, ASGI/WSGI runtime, or Granian replacement.

Initial README sections:

```text
What is eggserve?
Why not Python http.server?
Scope and non-goals
Expected CLI shape
Security defaults
Project status
Development
```

The project status should be explicit: early planning / pre-implementation until core milestones land.

### docs/threat-model.md

Document assets, trust boundaries, attacker capabilities, and default-deny behavior.

Assets:

```text
filesystem root confidentiality
filesystem root integrity
process availability
log integrity
host resource stability
operator expectation that public serving is intentional
```

Attacker capabilities:

```text
send arbitrary HTTP requests
send malformed request targets
use percent-encoded traversal attempts
hold connections open slowly
request large files repeatedly
attempt log injection through paths/headers
attempt symlink/reparse-point escape
attempt platform-specific path bypasses
```

Out-of-scope attacker capabilities for the initial version:

```text
local privileged attacker modifying served files concurrently
kernel/filesystem compromise
malicious operator-provided root directory that intentionally contains sensitive files
full reverse-proxy threat model
TLS certificate lifecycle automation
```

State the central invariant: under safe defaults, no remotely supplied request path may resolve to content outside the configured root, and no denied filesystem object class may be served.

### docs/security-policy.md

Define safe defaults:

```text
bind to loopback by default
GET and HEAD only
request bodies rejected
no symlink following by default
no dotfile serving by default
no directory listing by default unless explicitly changed
unknown MIME served as application/octet-stream
malformed request targets rejected
logs sanitized
resource limits enabled
```

Document unsafe or weaker options as explicit opt-ins. Use language such as `compatibility mode` or `unsafe convenience mode` only if those modes are clearly marked and not default.

### docs/non-goals.md

This document is important for resisting scope creep. Include at least:

```text
No ASGI or WSGI runtime
No dynamic Python callbacks in the initial server path
No CGI
No upload/write support in the initial product
No reverse proxying
No automatic ACME
No database-backed configuration
No plugin system
No templating engine
No authentication system except possible later basic-auth opt-in
No attempt to compete with nginx/Caddy as a full edge server
No attempt to compete with Granian/Uvicorn as app servers
```

### docs/architecture.md

Document the target workspace and module responsibilities. The important architectural point is separation:

```text
eggserve-core: security policy, path confinement, static serving, response construction
eggserve-bin: CLI, config loading, signal handling, startup policy display
eggserve-python: Python packaging and python -m launcher
```

The core crate should not depend on Python packaging concerns. The Python package should initially not own serving logic.

### docs/compatibility.md

Explain what compatibility with `http.server` means:

```text
similar command shape
similar simple local serving workflow
similar directory argument semantics where safe
not identical filesystem behavior
not identical directory listing defaults
not identical public bind behavior
not preserving unsafe traversal/symlink/dotfile behavior
```

Add a compatibility matrix stub for later:

```text
Feature | Python http.server | eggserve default | eggserve opt-in
Bind default | varies by invocation | loopback | public flag
Directory listing | enabled | disabled | --directory-listing
Symlinks | platform behavior | denied | --follow-symlinks
Methods | basic GET/HEAD | GET/HEAD | none initially
CGI | separate module | unsupported | unsupported
```

### docs/dependency-policy.md

Document dependency rules:

```text
Every dependency must have an explicit purpose.
No HTTP client stack for a server-only feature.
No web framework dependency in the initial milestones.
No templating dependency for generated directory listings.
No default TLS dependency before TLS milestone.
Feature flags must isolate optional surfaces.
Security-critical parsing dependencies require review.
```

List initially allowed dependency categories:

```text
tokio
hyper
hyper-util
http-body-util
bytes
minimal CLI parser
optional tracing
optional rustls later
```

### docs/release-criteria.md

Define alpha, beta, and 1.0 gates.

Alpha should require a functional CLI, safe defaults, and basic path regression tests. Beta should require fuzz targets and platform CI. 1.0 should require dependency audit, documented security review, Windows path coverage, resource-limit tests, and stable public API decisions.

## CI baseline

Add a minimal CI workflow if the repo uses GitHub Actions. Initial jobs:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If crates are only skeletal, keep CI simple but real. Add `cargo audit` and `cargo deny` in a later milestone once dependencies stabilize.

## Initial Rust workspace

Create minimal workspace files:

```toml
[workspace]
members = [
  "crates/eggserve-core",
  "crates/eggserve-bin",
]
resolver = "2"
```

`eggserve-python` may not be a Cargo crate initially if packaging is deferred. If it becomes a PyO3/maturin crate later, add it intentionally.

Create `eggserve-core` with placeholder modules:

```text
config.rs
policy.rs
limits.rs
error.rs
path.rs
```

These can contain skeletal types only. The goal is to make future work land in the intended boundaries.

## Acceptance criteria

This plan is complete when:

```text
The repo has a clear README and docs describing scope, threat model, security defaults, non-goals, architecture, dependency policy, compatibility, and release criteria.
The workspace skeleton exists and builds.
The first CI baseline exists or is intentionally deferred with a documented reason.
The project text consistently describes eggserve as a hardened static-serving foundation, not an application server.
No broad dependencies have been added prematurely.
```

## Risks

The main risk is scope creep. The second risk is over-documenting without giving future implementers concrete module boundaries. Keep docs precise and operational. Every non-goal should map to an implementation decision.

## Handoff notes

After this milestone, the next implementer should be able to start the Rust HTTP substrate without re-litigating product scope. If there is disagreement about whether a feature belongs in eggserve, require updating `docs/non-goals.md` and `docs/threat-model.md` before implementation.
