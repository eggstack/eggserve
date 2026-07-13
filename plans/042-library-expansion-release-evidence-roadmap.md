# Phase 42 — Library Expansion and Evidence-Driven Release Roadmap

## Purpose

Eggserve already has a strong hardened static-serving core, reusable filesystem and HTTP primitives, Rust-owned server I/O, Python bindings, raw-wire tests, fuzzing, packaging workflows, and a documented release contract. The next line of work is not to turn eggserve into a framework or a general application server. It is to complete the library boundary so downstream projects can safely build higher-level servers, adapters, download services, and protocol integrations without importing internal modules or reimplementing HTTP correctness.

In parallel, release readiness must move from an informal collection of workflow files and Markdown checkboxes to a repeatable, evidence-backed qualification system tied to one exact commit and one exact artifact set.

This roadmap therefore has two coordinated objectives:

1. expand the reusable Rust and Python library surface while preserving eggserve's narrow product identity;
2. replace release-readiness assertions with machine-readable gates, generated evidence, reproducible validation, and explicit human approval of a fixed release candidate.

## Product boundary

Eggserve remains:

- a hardened static file server;
- a reusable library for request validation, path confinement, filesystem resolution, response planning, file streaming, HTTP server lifecycle, and low-level client primitives;
- a Rust-owned transport runtime with optional Python callbacks;
- a foundation on which separate projects may build ASGI, WSGI, routing, middleware, authentication, or other application-server layers.

Eggserve does not become:

- an in-tree ASGI or WSGI implementation;
- a routing framework;
- a middleware framework;
- a reverse proxy;
- a session, authentication, cookie, template, upload, or multipart framework;
- a WebSocket runtime;
- a general replacement for nginx, Caddy, Uvicorn, Granian, or Hyper itself.

The design rule for every milestone is: expose correct, typed, transport-independent primitives; keep policy and I/O ownership in Rust; leave application semantics to downstream users.

## Starting state

Current strengths:

- secure static path parsing and confinement;
- descriptor-relative no-follow traversal on hardened Unix platforms;
- capability-style resolved files and directories;
- conditional request and range planning;
- GET/HEAD request-body rejection for the built-in static service;
- file-backed streaming without an eager Python copy;
- Rust and Python server primitives;
- lifecycle and boundary tests;
- raw-wire and production-path suites;
- property tests, fuzz targets, seed corpora, and corpus replay;
- package and wheel smoke infrastructure;
- action pinning, explicit permissions, dependency audit policy, and a release workflow;
- API stability inventory and release contract documentation.

Current gaps relevant to this roadmap:

- documentation and feature claims are not fully reconciled across README, non-goals, release contract, TLS documentation, platform claims, and package metadata;
- the public request model is not yet a complete transport-independent value model suitable for downstream application runtimes;
- duplicate-preserving header behavior is not consistent across Rust and Python;
- generic callback responses still leave some HTTP normalization obligations to handlers;
- bounded request-body primitives are absent because the built-in server is bodyless;
- the server runtime needs a more explicit builder, service, listener, readiness, and shutdown contract for downstream adapters;
- static service composition, safe prefix mounting, and explicit fallback outcomes remain limited;
- observability hooks are tied more closely to built-in logging than a reusable event contract;
- Python parity and typing need further development;
- the experimental client boundary needs cleanup after the server/library path is stable;
- release gates are documented but not represented by one machine-readable source of truth;
- local, CI, artifact, and human approval evidence are not yet assembled into one generated release bundle.

## Architectural principles

### Transport independence

Stable request, response, header, method, version, connection metadata, body, service, and error types must not expose Hyper, Tokio, PyO3, or workflow-specific implementation types.

### Safe construction

Malformed HTTP values should be unrepresentable through ordinary public constructors. Explicit raw or unchecked APIs, if retained, must remain internal or experimental.

### Capability preservation

Securely resolved files must remain opened capabilities. Downstream consumers must not be forced to convert a resolved resource back into a path and reopen it.

### Rust-owned I/O

Socket parsing, framing, timeouts, connection accounting, file streaming, and shutdown cancellation remain owned by Rust. Python and downstream handlers operate through bounded value and body abstractions.

### Static defaults remain strict

The built-in static server continues to accept only GET and HEAD and continues to reject request bodies. Broader library primitives must not silently broaden CLI behavior.

### Scope by composition, not framework features

Eggserve may expose a minimal service trait, fallback outcome, method guard, or safe prefix mount. It must not add a route table, middleware registry, dependency injection, sessions, templates, or application framework conventions.

### Evidence over declarations

A workflow job existing in YAML does not close a release gate. Release evidence must identify the exact commit, exact job execution, exact artifacts, and exact results.

## Milestone sequence

## Milestone 1 — Contract and release infrastructure

Goal: establish a coherent product contract and a machine-enforced release qualification foundation before expanding stable APIs.

Work:

- reconcile scope and support claims across README, non-goals, release contract, security policy, API stability inventory, Python packaging metadata, Rust package metadata, TLS documentation, and platform support tables;
- define pre-1.0 compatibility guarantees and the stability meaning of stable, experimental, and internal;
- publish a Rust/Python capability and parity matrix;
- define minimum supported Rust and exact supported Python versions;
- create a machine-readable release criteria manifest;
- assign stable identifiers to every required release gate;
- add a unified local validation entry point that emits structured evidence;
- normalize GitHub Actions job names around those gate identifiers;
- define evidence freshness and invalidation rules;
- generate the human-readable release checklist from the criteria/evidence model rather than maintaining an independent checklist by hand.

Detailed handoff plans:

- `043-milestone-1-contract-scope-reconciliation.md`
- `044-milestone-1-machine-readable-release-criteria.md`
- `045-milestone-1-unified-validation-ci-gates.md`

Exit criteria:

- no material contradiction exists among public scope, feature, platform, TLS, packaging, and support claims;
- every public export has a stability tier and support expectation;
- every advertised platform/version maps to a required test gate;
- one criteria file is the source of truth for release gates;
- local and CI validation use the same stable gate identifiers;
- release checklist output can be generated from structured evidence;
- existing release workflows remain publication-safe and dry-run by default.

## Milestone 2 — Canonical HTTP value types

Goal: create complete transport-independent request and response value models shared by Rust, the built-in server, Python bindings, and downstream consumers.

Work:

- typed method model with standard and extension methods;
- HTTP version model;
- immutable request head;
- ordered duplicate-preserving headers;
- validated header names and values;
- safe single-value and multi-value lookup;
- connection metadata separated from proxy headers;
- response builder with validated status and headers;
- deterministic Content-Length handling;
- automatic HEAD suppression while preserving representation metadata;
- body-forbidden status normalization for 1xx, 204, and 304;
- rejection of invalid framing and user-supplied transfer coding;
- Rust/Python parity and conformance fixtures.

Exit criteria:

- downstream code can inspect and construct HTTP messages without Hyper types;
- duplicate headers survive both Rust and Python boundaries;
- common response-framing mistakes cannot be produced through safe APIs;
- built-in static responses and callback responses converge on one normalization path.

## Milestone 3 — Reusable server runtime and lifecycle

Goal: make the Rust-owned HTTP runtime cleanly embeddable by downstream projects.

Work:

- stable server builder;
- stable service boundary;
- bind by address or existing listener;
- bound-address inspection and readiness notification;
- explicit connection, request, body, and file-stream limits;
- header, body, handler, write, TLS handshake, and shutdown timeouts;
- graceful and forced shutdown;
- per-connection cancellation;
- panic and callback exception containment;
- plaintext/TLS lifecycle parity;
- sustained keep-alive and shutdown race tests.

Exit criteria:

- a downstream crate can build an application-server adapter using only stable public APIs;
- transport ownership remains in Rust;
- lifecycle behavior is deterministic under startup races, cancellation, failure, and shutdown.

## Milestone 4 — Bounded request bodies

Goal: support downstream dynamic services without weakening the bodyless built-in static server.

Work:

- request body policy: reject, buffer up to limit, stream up to limit;
- transfer-decoded Rust-owned body stream;
- fixed-length and chunked accounting;
- body timeout and cancellation;
- one-shot consumption semantics;
- premature EOF and overrun errors;
- drain-or-close policy after incomplete consumption;
- Python read and chunk iteration APIs;
- disconnect, timeout, limit, and cancellation tests.

Exit criteria:

- downstream services can safely consume bounded bodies;
- chunked coding cannot bypass limits;
- built-in static serving remains GET/HEAD and bodyless;
- connection behavior after partial body consumption is deterministic.

## Milestone 5 — Static service composition and filesystem completion

Goal: make hardened static serving a reusable service component rather than a CLI-only path.

Work:

- static service builder;
- explicit handled/not-handled outcome separate from HTTP 404;
- dynamic-to-static fallback;
- safe path-prefix mount;
- method guard with correct Allow header;
- configurable index files;
- MIME resolver interface with octet-stream fallback and no sniffing by default;
- cache metadata policy for ETag, Last-Modified, Cache-Control, and immutable assets;
- stable filesystem metadata and denial taxonomy;
- safe child resolution and pre-opened file response generation;
- race, replacement, truncation, unlink, permission-change, and non-UTF-8 tests.

Exit criteria:

- common static-plus-dynamic servers require no private APIs;
- path transformations preserve confinement guarantees;
- resolved capabilities are never reopened by pathname;
- filesystem outcomes are typed and stable.

## Milestone 6 — Python parity and ergonomics

Goal: project the stabilized Rust contracts into a coherent Python library.

Work:

- canonical Request, Response, Headers, ConnectionInfo, body, service, and server types;
- ordered duplicate-preserving header API;
- file-backed and streaming responses;
- stable exception hierarchy;
- server builder and lifecycle controls;
- explicit synchronous handler scheduling model;
- bounded worker execution and GIL release around I/O;
- async handler support only as a separate experimental contract if implemented;
- `.pyi` stubs or equivalent generated typing;
- clean-wheel cross-platform tests and API snapshots.

Exit criteria:

- Python and Rust expose the same conceptual model;
- source-tree imports are absent from release validation;
- callback exceptions, shutdown, and large file responses are deterministic;
- stable Python signatures and exceptions are machine-tested.

## Milestone 7 — Observability and compatibility enforcement

Goal: enable production instrumentation and prevent accidental API or semantic drift.

Work:

- sanitized observer events for connections, requests, responses, denials, timeouts, failures, and shutdown;
- no-op observer and optional tracing adapter;
- bounded/non-blocking Python event delivery where supported;
- Rust compile fixtures for stable APIs;
- semver/public API checks;
- Python `__all__`, signatures, exception, enum, and type-stub snapshots;
- shared Rust/Python semantic conformance corpus;
- feature matrix, no-default-features, MSRV, and downstream example builds.

Exit criteria:

- downstream users can instrument eggserve without adopting a logging framework;
- observer failures cannot fail requests;
- accidental stable API drift fails CI;
- behavioral changes require intentional conformance updates.

## Milestone 8 — Production qualification and release bundle

Goal: turn production-hardening claims into measurable release evidence.

Work:

- slow-header, slow-reader, connection-saturation, stream-saturation, cancellation, lifecycle, filesystem-race, TLS timeout, and callback-failure qualification;
- memory, file descriptor, task, permit, and shutdown plateau assertions;
- cross-platform artifact build/install tests;
- generated release evidence bundle containing manifest, gate results, checksums, provenance, artifact inventory, limitations, and checklist;
- dry-run release execution and artifact inspection;
- exact-SHA human approval;
- staged publication and public-registry post-release smoke tests;
- yank/recovery/security response procedures.

Exit criteria:

- every mandatory gate has current evidence tied to one SHA;
- every advertised artifact is installed and tested outside the checkout;
- security/resource qualification passes documented thresholds;
- publication consumes previously qualified artifacts rather than rebuilding them;
- post-publication installation succeeds from public registries.

## Milestone 9 — Experimental client cleanup

Goal: keep the client useful for interoperability and low-level consumers without expanding eggserve into an HTTPX/requests replacement.

Work:

- canonical request/response/header types shared with the server where appropriate;
- bounded or streaming response body;
- explicit redirect policy, disabled by default;
- TLS verification by default;
- timeout phase taxonomy;
- no implicit retries, cookies, proxy environment interpretation, or decompression;
- feature and crate-boundary review.

Exit criteria:

- the client supports conformance tests and explicit low-level use;
- client instability does not block the stable server/static library release;
- all client APIs remain clearly experimental until separately promoted.

## Release criteria model

The release system should distinguish four evidence classes:

- `LOCAL`: exact command, tool versions, platform, commit, dirty state, result, and timestamp;
- `GITHUB`: workflow, run, job, commit, result, artifact, and timestamp;
- `ARTIFACT`: digest, target, package metadata, provenance, and independent smoke result;
- `HUMAN`: approver, date, exact commit, evidence-bundle digest, limitations, and waivers.

Configuration review alone does not close an execution gate.

Each gate should define:

- stable ID;
- description;
- required/advisory status;
- applicable platforms/features;
- command or workflow job;
- expected artifacts;
- maximum evidence age where relevant;
- release-critical files that invalidate the result;
- dependencies on other gates;
- accepted waiver policy.

## Required release gate families

At minimum:

- Rust formatting, lint, tests, doctests, MSRV, no-default-features, and feature matrices;
- HTTP raw-wire and production-path correctness;
- filesystem security and race tests;
- property and corpus replay tests;
- current fuzz campaign evidence;
- TLS client and server validation;
- dependency audit and license policy;
- Rust package and publish-dry-run validation;
- Python API, typing, wheel, and installed-smoke validation;
- Linux, macOS, and Windows artifact validation with explicit hardening classification;
- API compatibility and semantic conformance;
- documentation and metadata consistency;
- resource and shutdown qualification;
- release dry run, checksums, provenance, and publication gating;
- public-registry post-release smoke tests.

## Completion definition

This roadmap is complete when a separate downstream project can, using stable public APIs only:

- accept HTTP connections through the eggserve runtime;
- inspect a transport-independent request;
- consume a bounded request body;
- construct an HTTP-correct response;
- stream bytes or an already-resolved file;
- mount hardened static serving as a fallback;
- enforce its own application routing and semantics externally;
- observe lifecycle and request events;
- shut down cleanly;
- implement an ASGI, WSGI, or other adapter without importing eggserve internals.

The release process is complete when one exact commit has a generated evidence bundle proving that all required gates passed, every advertised artifact was independently installed and exercised, current fuzz and robustness evidence exists, limitations are explicit, checksums and provenance are verified, and a human approved the exact candidate and evidence digest.

## Immediate next work

Implement Milestone 1 through plans 043–045 before changing stable request, response, or runtime APIs. This prevents later work from expanding against an inconsistent contract or an unstructured release process.