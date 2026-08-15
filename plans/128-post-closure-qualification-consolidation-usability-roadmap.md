# Plan 128 — Post-Closure Qualification, Consolidation, and Usability Roadmap

## Status

**COMPLETE — 2026-08-15.**

Reviewed baseline:

```text
main = 4a2371045e221c6d3875f3a6085bd67fa53de7f5
```

This plan starts a new, explicitly user-authorized post-closure track after Plans 126–127. Plan 127's instruction not to create another corrective plan applied to that closed corrective track; it does not prohibit this newly requested qualification/consolidation/usability track.

This roadmap must not reopen EggServe's architecture or broaden product scope. The implementation is now substantially complete for its intended purpose. The work here is to prove the product on supported surfaces, reduce repository/process weight, make the documentation and examples accurately communicate what exists, and ensure the Rust library and CLI are usable first-class entry points alongside the Python API.

---

## Product statement for this track

EggServe should finish this track with one clear product identity:

> EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped facade. The CLI is static-serving focused. The Rust and Python libraries expose bounded primitives and custom response handling without becoming an ASGI/WSGI runtime, application framework, proxy, or general-purpose edge server.

The intended surfaces are:

1. **CLI executable** — a secure-by-default replacement for `python -m http.server` for file serving.
2. **Python `eggserve.server` facade** — source-familiar `HTTPServer`/`SimpleHTTPRequestHandler` and bounded `BaseHTTPRequestHandler` usage.
3. **Python convenience APIs** — `serve_directory()` and optional subprocess lifecycle helpers.
4. **Rust `eggserve-core` library** — reusable hardened static-serving primitives and an embeddable HTTP/1 runtime/service boundary.
5. **Rust CLI crate** — executable behavior remains thin over the library and does not duplicate HTTP/filesystem policy.

The Python surface remains the primary compatibility target, but the Rust library must not feel like an undocumented internal dependency.

---

## Scope of the roadmap

This roadmap is split into five execution plans:

```text
Plan 129 — Platform and product qualification
Plan 130 — Repository deletion, consolidation, and verification simplification
Plan 131 — Documentation and compatibility-contract polish
Plan 132 — Executable examples and product demonstrations
Plan 133 — Rust library and CLI usability closure
```

These plans may be implemented sequentially or in tightly coordinated branches, but closure should follow the ordering above because later documentation/examples should describe the final consolidated structure rather than stale pre-cleanup state.

---

## Governing principles

### 1. Qualification before claims

Do not strengthen support language because tests exist in source. Run the relevant tests on the platform/product surface being claimed. Windows adversarial qualification remains the most important open qualification item.

### 2. Prefer deletion over abstraction

This track is explicitly allowed to remove stale scripts, duplicated workflow bodies, redundant documentation, obsolete feature flags, abandoned compatibility scaffolding, and historical artifacts that no longer provide maintenance value.

Do not replace deleted complexity with a new framework, registry, generator, or orchestration layer.

### 3. Keep routine CI small

Routine CI should remain approximately its current shape: one Rust regression job and one Python wheel regression job on Linux. Expensive/platform-specific qualification belongs in manual/local verification or manually dispatched workflows.

### 4. Preserve the security ownership boundary

Do not expose raw sockets, authoritative translated filesystem paths, reopened path-based static responses, or other compatibility escape hatches merely to make examples look more stdlib-like.

### 5. Rust usability means documented and executable, not framework expansion

The Rust library should support straightforward static embedding and a small custom service example using the existing `Server`, `RuntimeConfig`, `StaticService`, `Service`/`service_fn`, canonical request/response types, and hardened primitives.

Do not create an ASGI-like Rust framework, router, middleware stack, templating layer, websocket subsystem, HTTP client, proxy, or application runtime.

### 6. Examples are verification assets

Every canonical example added by this track must either compile/run in a normal verification path or have a deterministic smoke procedure. Examples must not become aspirational snippets that drift from the API.

---

## Phase dependencies

### Phase 1 — Plan 129 qualification

Run the existing product against real platform/runtime boundaries before changing support language. Record failures as concrete corrective items rather than redesign invitations.

Required outcomes:

- Linux static CLI and Python facade qualified from installed/package-like artifacts;
- macOS static CLI/Python behavior qualified on a real macOS host or existing supported CI runner where practical;
- Windows ordinary runtime plus the existing adversarial filesystem/confinement suite executed on a real Windows environment, preferably a manually dispatched GitHub-hosted Windows runner if no local Windows machine is available;
- Rust library consumer build/run qualification from outside the workspace;
- support statements updated only after evidence exists.

### Phase 2 — Plan 130 deletion/consolidation

Use qualification results and repository inventory to remove obsolete machinery and collapse duplicated verification/release code.

Required outcomes:

- duplicated manual release workflow logic materially reduced;
- stale plan-era feature flags/helpers/scripts removed if unreferenced;
- verification scripts have a small, documented purpose split;
- no routine CI expansion;
- no functional/security regression.

### Phase 3 — Plan 131 docs polish

After structural cleanup, rewrite only the normative docs necessary to present the final product accurately.

Required outcomes:

- one consistent product statement;
- a concise surface/compatibility matrix;
- CLI, Python, and Rust entry points discoverable from README;
- `http.server` compatibility claims are precise about supported vs intentionally unsupported behavior;
- Windows security posture matches actual qualification evidence.

### Phase 4 — Plan 132 examples

Create a compact set of executable examples that demonstrate the intended product, including canonical Python `http.server` replacement usage and real Rust embedding.

Required outcomes:

- CLI static-server walkthrough;
- Python stock static facade example;
- Python custom handler example;
- Rust static server example;
- Rust custom service example;
- optional primitive-only safe file/download example if it demonstrates a distinct security boundary;
- examples are verified without adding heavy CI.

### Phase 5 — Plan 133 Rust/CLI usability closure

Audit the Rust package boundary and CLI ergonomics from the perspective of an external consumer. Fix only concrete friction that prevents straightforward use.

Required outcomes:

- `eggserve-core` package can be consumed from a clean external crate;
- public Rust docs identify stable-ish vs experimental surfaces correctly;
- simple static serving does not require internal modules or direct Hyper usage;
- custom service example uses only supported public modules;
- CLI installation and invocation paths are documented and smoke-tested;
- no unnecessary facade crate is introduced unless a concrete packaging blocker proves one necessary.

---

## Cross-track acceptance criteria

The entire roadmap is complete only when all are true:

- [x] routine CI remains small and green;
- [x] manual/deep verification remains manual rather than becoming a required push gate;
- [x] Windows support language is backed by actual Windows execution evidence;
- [x] CLI remains secure-by-default and static-serving focused;
- [x] Python `http.server`-shaped examples work from the installed wheel;
- [x] stock `SimpleHTTPRequestHandler` still uses the native fast path under its documented eligibility contract;
- [x] Rust static serving works from a clean external consumer crate using public API only;
- [x] Rust custom service handling works without importing Hyper directly;
- [x] no raw-socket or raw translated-path compatibility escape hatch is added;
- [x] duplicated release workflow logic is reduced rather than expanded;
- [x] obsolete plan-era implementation artifacts identified by Plan 130 are removed or explicitly retained with rationale;
- [x] README presents CLI, Python, and Rust usage without contradictory product claims;
- [x] examples compile/run under documented verification commands;
- [x] no new application-serving scope is introduced;
- [x] final documentation describes actual behavior rather than planning history.

---

## Explicit non-goals

This roadmap must not add:

- ASGI or WSGI adapters;
- HTTP/2 or HTTP/3 solely for parity marketing;
- reverse-proxy behavior;
- HTTP client functionality;
- websocket support;
- virtual hosting;
- ACME/certificate automation;
- templating/routing/middleware frameworks;
- a generalized plugin system;
- raw socket exposure in Python;
- authoritative `translate_path()` behavior that reintroduces path reopening;
- automatic GitHub release publication;
- benchmark gates in routine CI;
- a broad OS matrix in routine CI;
- generated verification registries or evidence databases;
- a new top-level Rust facade crate merely for naming aesthetics.

---

## Verification posture for the roadmap

Routine development verification should continue to use the project's existing small screen:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Full/deep/manual checks should be selected by the plan being implemented. Do not run every expensive suite for documentation-only changes.

Where examples are added, prefer extending `scripts/verify.sh full` or a narrowly named manual example-smoke script rather than adding new routine GitHub Actions jobs.

---

## Closure record requirements

When Plans 129–133 are complete, append a concise closure record to this file containing:

```text
final main commit
Plan 129 qualification evidence summary
Plan 130 deleted/consolidated inventory summary
Plan 131 normative docs changed
Plan 132 examples added and verification command/results
Plan 133 external Rust consumer and CLI qualification results
final routine CI run
remaining known limitations, if any
```

Do not create another broad roadmap for cosmetic leftovers. Any remaining item should be either a concrete bug/security issue or ordinary maintenance.

## Closure record — 2026-08-15

- Final main commit: `3ef6a5ab9df5b393a9f4bf84cf600d01d3cd0e08` (Plan 133 implementation and documentation closure).
- Plan 129: Linux/macOS product qualification, Windows Outcome 2 evidence, clean external Rust static/custom consumers, and package dry-run are recorded in [Plan 129](129-platform-and-product-qualification.md).
- Plan 130: repository deletion/consolidation and verification simplification are recorded in [Plan 130](130-repository-deletion-consolidation-verification-simplification.md).
- Plan 131: normative README, compatibility, capability-matrix, and support-document updates are recorded in [Plan 131](131-documentation-and-compatibility-contract-polish.md).
- Plan 132: canonical CLI, Python, and Rust examples plus executable example verification are recorded in [Plan 132](132-executable-examples-and-product-demonstrations.md).
- Plan 133: Rust public API/rustdoc, CLI integration boundary, package usability, README/AGENTS/skill/architecture cleanup, and external consumer evidence are recorded in [Plan 133](133-rust-library-and-cli-usability-closure.md).
- Final routine CI: [run 31868456917](https://github.com/eggstack/eggserve/actions/runs/31868456917), both `rust` and `python` jobs passed.
- Remaining limitations: Windows remains functionally qualified and trusted/local-content only because the two NTFS path-rename cases documented by Plan 129 remain skipped; the Rust `server` module remains experimental before 1.0.
