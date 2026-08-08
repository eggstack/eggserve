# Plan 112 — Scope Consolidation and Simplification Roadmap

## Status

**PLANNED — 2026-08-08.**

This roadmap starts a bounded consolidation track after the verified hardening and runtime-correctness work through Plans 109–111.

The repository is functionally close to its intended product: a hardened, HTTP-correct static file server with an `http.server`-shaped Python facade and reusable HTTP/security primitives. The remaining work is primarily subtraction, truthfulness, and release-surface consolidation rather than new capability.

This roadmap authorizes Plans 113–118 only. It does not reopen the earlier hardening roadmap and does not authorize a new feature track.

---

## Product definition to preserve

EggServe remains:

```text
hardened static file server
    + safe-by-default filesystem confinement
    + correct HTTP/1.1 static response semantics
    + reusable Rust HTTP/security primitives
    + bounded Python http.server-shaped facade
    + optional native TLS where already supported
```

EggServe is not:

```text
ASGI/WSGI application server
reverse proxy
HTTP/2 or HTTP/3 stack
ACME/virtual-hosting platform
web framework
plugin host
full HTTP client library
requests/httpx replacement
general application runtime
```

Downstream projects may use EggServe primitives to build application servers, clients, adapters, or other HTTP software. Those downstream products do not need to be implemented inside this repository.

---

## Why this track exists

The current implementation has good core architecture but retains accumulated surface from the hardening/build-out period:

1. a feature-gated HTTP client subsystem despite the static-server product boundary;
2. a Python `client.rs` source file that is not part of the currently compiled top-level binding surface and whose required core client feature is not enabled by the Python manifest;
3. a deprecated pre-runtime `service` compatibility adapter after production serving moved to `Server` / `StaticService` / `RuntimeState`;
4. direct binary-crate dependencies whose visible use is test-only or redundant with `eggserve-core`;
5. an intentionally small routine CI workflow surrounded by a much larger verification taxonomy and overlapping test documentation;
6. overlapping architecture/reference documentation that has repeatedly drifted out of sync;
7. timeout documentation that can imply a distinct response-write timeout even though the exposed runtime configuration is centered on header, handler, body-read, and total-connection deadlines;
8. a large error taxonomy whose size should be reassessed after out-of-scope surfaces are removed;
9. Python distribution choices that currently force a narrow CPython 3.14 range and compile TLS into the native extension;
10. recorded binary-size work that should be used as a guardrail, not turned into a permanent benchmark bureaucracy.

The objective is to reduce code, dependency, verification, and documentation surface without weakening the hardened filesystem or HTTP correctness model.

---

## Non-negotiable invariants

Every implementation plan in this roadmap must preserve all of the following.

### Filesystem security

- configured-root confinement remains enforced at the library level;
- safe-default Unix traversal remains descriptor-relative;
- safe-default Windows traversal remains handle-relative at its current qualification level;
- dotfiles remain denied by default;
- symlinks remain denied by default;
- directory listing remains disabled by default;
- file-backed responses retain opened-handle/capability semantics through the transport boundary;
- no simplification may reintroduce pathname reopen races.

### HTTP correctness

- HTTP/1.1 remains the supported transport;
- GET/HEAD static semantics remain correct;
- conditional requests remain correct;
- single-range behavior remains correct;
- HEAD representation metadata remains equivalent to GET while omitting the body;
- response normalization remains authoritative for body-forbidden status codes, `Content-Length`, hop-by-hop handling, and `Date` behavior;
- request framing/body-policy hardening remains intact.

### Runtime ownership

- the running server owns connection admission;
- the running server owns the single file-stream admission pool;
- static services do not create independent transport admission pools;
- Python callbacks do not receive raw sockets or reopen translated static paths;
- production serving continues through the canonical runtime service boundary.

### Scope discipline

- no new protocol versions;
- no new application-serving adapters;
- no proxy functionality;
- no new dependency merely to perform cleanup;
- no additional CI gate unless removal of an existing gate would otherwise create a concrete correctness blind spot;
- no benchmark framework expansion.

---

## Roadmap sequence

Execute in order unless a plan explicitly states that it may overlap.

### Plan 113 — Product-surface consolidation

Purpose:

- resolve the HTTP client subsystem against the actual product contract;
- remove orphaned/dead Python client binding source if it is not a supported release surface;
- remove the deprecated pre-runtime service compatibility adapter when no supported consumer requires it;
- remove tests/docs whose only purpose is preserving deleted compatibility surfaces;
- keep generic canonical HTTP primitives required by servers or downstream consumers.

Default decision: EggServe should expose primitives sufficient for downstream client implementations, but should not itself ship a separate HTTP client product. If repository evidence contradicts that decision by showing a supported published contract, stop that deletion and record the compatibility constraint rather than silently breaking it.

### Plan 114 — Dependency and artifact slimming

Purpose:

- make manifests reflect actual production ownership;
- demote test-only dependencies;
- remove unused direct dependencies and accidental feature activation;
- measure default/TLS CLI and Python extension/wheel artifacts before and after;
- keep only size reductions that do not reduce supported functionality or worsen clarity.

This is dependency hygiene first, binary-size optimization second.

### Plan 115 — Verification and CI consolidation

Purpose:

- retain the current small two-job CI posture;
- reduce duplicated command definitions and overlapping verification tiers;
- distinguish routine regression checks, release checks, and subsystem-specific diagnostic tools;
- retain fuzz/race/fault/proxy assets where they have security value without making them permanent merge gates.

The target is simpler verification semantics, not fewer correctness tests merely for the sake of count reduction.

### Plan 116 — Runtime and API semantic cleanup

Purpose:

- reconcile `connection_total_timeout` semantics and documentation;
- prefer truthful documentation over adding another timer unless a demonstrated stalled-write defect requires one;
- reassess overlapping error families after Plan 113 removes dead/out-of-scope surfaces;
- simplify error conversions only where distinctions are redundant;
- preserve security-significant and externally useful error distinctions.

### Plan 117 — Python distribution and compatibility cleanup

Purpose:

- audit why the native extension unconditionally includes TLS-related core/dependencies;
- avoid duplicated TLS/dependency ownership where possible;
- evaluate broader Python-version support, preferably through a stable PyO3/abi3-compatible approach if feasible;
- preserve the `http.server`-shaped API and wheel-installed CLI behavior;
- avoid creating a large wheel matrix or automated release pipeline.

Python-version broadening is conditional on low complexity and demonstrable compatibility. It is not allowed to become a packaging project in its own right.

### Plan 118 — Documentation consolidation and closure verification

Purpose:

- reduce duplicate normative documentation;
- establish one authoritative source per policy/architecture fact;
- fix known truthfulness defects such as obsolete audit/CI claims;
- archive or clearly label historical planning records without rewriting history;
- perform one final targeted verification pass over the simplified product surface;
- close this roadmap without creating another documentation-only follow-up cycle for minor prose issues.

---

## Explicitly protected code areas

The following areas are not cleanup targets unless a concrete defect is found while executing a plan:

```text
crates/eggserve-core/src/fs/
crates/eggserve-core/src/path/
crates/eggserve-core/src/server/static_service.rs
canonical response normalization
file-backed BodySource / ResponseBody transport boundary
runtime-owned file-stream admission
Windows handle-relative confinement implementation
Unix descriptor-relative confinement implementation
```

Do not simplify these components merely because they are complex. Their complexity corresponds to actual security or HTTP invariants.

---

## What may be deleted aggressively

Deletion is encouraged when all supported consumers and tests show a surface is obsolete:

```text
orphaned Python client bindings
feature-gated full HTTP client implementation if not part of the release contract
client-only tests/docs/features after client removal
deprecated pre-runtime service adapter
legacy test helpers that only exercise the removed adapter
unused or test-only direct dependencies
redundant verification wrappers
stale duplicated architecture prose
obsolete plan-status references in active docs
```

Historical plan files remain records and should not be deleted merely because their implementation is obsolete.

---

## Measurement policy

Do not optimize based on intuition alone.

Before and after changes that may affect artifacts, record:

```sh
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-core --no-default-features
cargo build --profile dist --locked -p eggserve-bin
cargo build --profile dist --locked -p eggserve-bin --features tls
```

For Python, use the repository-supported wheel build/test path and measure:

- native extension size;
- bundled CLI size;
- compressed wheel size.

Use Plan 109 measurements as historical comparison only. Do not fail a change because toolchain changes produce small size movement. A meaningful regression is one explained by newly retained code/features, not normal compiler/linker variation.

Do not add a permanent binary-size CI gate.

---

## Verification philosophy for this roadmap

Verification must be proportional to the changed subsystem.

Routine baseline:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
bash scripts/test-python-wheel.sh
```

Feature-specific checks are added only when the implementation touched that feature.

Filesystem race, fault injection, fuzz replay, proxy interoperability, and TLS abuse tests remain valuable but are selected by change risk. They are not all mandatory for manifest/docs-only changes.

No phase may create a new gate registry, generated checklist system, evidence-upload workflow, or release automation.

---

## Acceptance criteria for the complete roadmap

This roadmap is complete only when all of the following are true.

### Product surface

- the supported product can be described without contradiction as a hardened static server plus reusable HTTP/security primitives and Python `http.server` facade;
- no dead Python client source remains presented as active architecture;
- no full HTTP client implementation remains unless explicitly retained as an intentional supported contract with a documented rationale;
- the deprecated pre-runtime service adapter is removed unless a concrete supported consumer requires it;
- only one production serving architecture remains.

### Dependencies and artifacts

- each direct dependency has a current production, platform, build, or test purpose;
- test-only dependencies are not normal dependencies without a reason;
- default CLI remains approximately within the existing sub-megabyte `dist` class unless compiler/toolchain variance explains otherwise;
- TLS remains optional for the standalone CLI;
- no functionality is removed solely for binary size.

### Verification

- routine CI remains small and comprehensible;
- local verification has a clear routine/release distinction;
- expensive security/diagnostic tests remain runnable without being universal gates;
- CI does not publish releases.

### Runtime/API

- timeout behavior and documentation describe the same semantics;
- no redundant error family remains solely because a removed subsystem once required it;
- path/body/response errors retain distinctions needed for security and public behavior.

### Python

- the Python manifest and compiled modules agree about enabled functionality;
- TLS dependency ownership is intentional and documented;
- Python-version support is broadened if that can be achieved without disproportionate packaging complexity, otherwise the 3.14-only constraint is explicitly justified as temporary;
- installed-wheel behavior remains verified.

### Documentation

- active docs no longer claim `cargo audit`/`cargo deny` run in routine CI unless they actually do;
- one normative document owns each major policy area;
- architecture/reference docs do not describe deleted surfaces as current;
- Plans 112–118 are recorded as the bounded consolidation track;
- no additional closure plan is required for known issues from this roadmap.

---

## Rejection conditions

Reject an implementation under this roadmap if it:

- weakens filesystem confinement to make code smaller;
- replaces opened-handle file serving with checked-path-then-reopen behavior;
- removes HTTP correctness tests while changing HTTP semantics;
- adds ASGI/WSGI, proxy, HTTP/2, HTTP/3, ACME, auth, upload, or framework behavior;
- creates a new generalized HTTP client to replace the client subsystem being removed;
- adds dependencies for convenience during dependency-reduction work;
- introduces a complex Python wheel/release matrix;
- turns size measurements into a mandatory CI benchmark gate;
- turns fuzz/race/fault/proxy suites back into universal PR gates;
- rewrites security-significant error variants merely to reduce type count;
- treats historical plan documents as normative current architecture;
- creates further planning phases beyond Plan 118 without a concrete defect that prevents closure.

---

## Handoff order

Implement in this sequence:

```text
112 roadmap (this file)
  -> 113 product-surface consolidation
  -> 114 dependency and artifact slimming
  -> 115 verification and CI consolidation
  -> 116 runtime/API semantic cleanup
  -> 117 Python distribution compatibility cleanup
  -> 118 documentation consolidation and closure verification
```

Plans 114 and 115 may be prepared in parallel after Plan 113's deletion decisions are known, but their final measurements/verification configuration must use the post-113 tree.

Plan 118 is the closure gate for this roadmap.
