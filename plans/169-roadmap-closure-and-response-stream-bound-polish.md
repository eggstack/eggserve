# Plan 169 — Roadmap Closure and Response-Stream Bound Polish

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plans 162–168 have landed in substance. Plan 170 owns the remaining broad performance-evidence work and final performance-claim closure.

## Goal

Close the narrow correctness/documentation/API-polish gaps left after implementation of the production/embedding/anonymity roadmap without reopening completed runtime work.

This plan has two purposes:

1. reconcile Plans 161–168 with the implementation/commit history so the planning record no longer says implemented work is still “ready”; and
2. review and, if technically valid, relax the unnecessary `Sync` requirement on transport-independent response streams so downstream application servers can use ordinary one-owner `Send` producers.

This is a closure plan, not a new feature phase.

## Current evidence

The implementation history is authoritative:

- Plan 162 — streaming responses: `e132e3766ed5440ddcd865f8845cabc3142550c6`;
- Plan 163 — transport-neutral connection driver: `5aa126183b1859d290a0d67713fd5668e424d9c7`;
- Plan 164 — production admission/parser/lifecycle controls: `888e24f9c636b765a8bf808b142a71da5b73d687`;
- Plan 165 — response privacy/fingerprint policy: `7c63911e45727e9c5ccf5ff1e9f3639530f14c81`;
- Plan 166 — Python low-level runtime/service substrate: `fcb39f12abcf37298442013d5c96e58ae4f37120`;
- Plan 167 — CGI/FastCGI gate closed as no-go: `5083252cc9b4b6bf65f23caaa361c910912f8d87`;
- Plan 168 qualification/correctness closure: `b2462a6df2e571c1fc85bf637601a761469b38e2`;
- Plan 168 evidence-SHA correction: `4922227f55502885621620aaa4c915458055ec84`.

Current `main` also contains dedicated `response_streaming`, `transport_driver`, `production_controls`, and `response_privacy` suites plus the Python low-level runtime suite and public examples.

The historical plan files were not consistently updated after implementation: for example Plan 162 still says `READY FOR IMPLEMENTATION`, and Plan 168 still presents pre-implementation status/checklists. That is planning-state drift, not missing implementation.

## Track A — Reconcile the planning record

### Preserve history; correct status

Do not rewrite old plan bodies to pretend the work was known in advance. Historical rationale, proposed API names, checklists, and design discussion remain historical.

For Plans 161–168:

- update only the top-level `## Status` section as needed;
- append a short `## Closure record` section when useful;
- record the implementation/no-go commit SHA(s);
- identify any material deviation from the proposed plan;
- point unresolved performance qualification to Plan 170 rather than leaving ambiguous unchecked boxes.

Recommended status outcomes before Plan 170:

- 162: **IMPLEMENTED / CLOSED**;
- 163: **IMPLEMENTED / CLOSED**;
- 164: **IMPLEMENTED / CLOSED**;
- 165: **IMPLEMENTED / CLOSED**;
- 166: **IMPLEMENTED / CLOSED**;
- 167: **CLOSED — NO-GO, no in-tree CGI/FastCGI adapters**;
- 168: **IMPLEMENTED FOR CORRECTNESS/RESOURCE/PRIVACY QUALIFICATION; PERFORMANCE-EVIDENCE EXTENSION TRACKED BY PLAN 170**;
- 161: **IMPLEMENTED IN SUBSTANCE; FINAL ROADMAP CLOSURE PENDING PLAN 170**.

Plan 170 performs the final 161/168 closure update once the evidence scope is resolved.

### Authoritative-doc audit

Audit current-state documentation for stale language introduced by the old statuses. At minimum check:

- `README.md`;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- `docs/api-stability.md`;
- `docs/public-api-boundary.md`;
- `docs/library-capability-matrix.md`;
- `docs/extension-contract.md`;
- `architecture/runtime.md`;
- `architecture/testing-and-conformance.md`;
- `benchmarks/README.md`.

Do not change product claims beyond the evidence. In particular, keep the distinction between deterministic qualification coverage and the narrower currently-recorded throughput snapshot.

## Track B — Review the `ResponseStream: Sync` requirement

### Current contract

`ResponseStream` currently stores:

```rust
Pin<Box<dyn Stream<Item = Result<Bytes, ResponseStreamError>> + Send + Sync>>
```

and its constructors require producer streams to be `Send + Sync + 'static`.

That is stricter than the conceptual ownership model: a response body is one-shot and is polled by its connection task. A downstream app server should not have to make a producer concurrently shareable merely to stream a response.

### Upstream constraint check

At implementation time re-check the locked/current versions, but the relevant current upstream contracts are:

- Hyper HTTP/1 `Builder::serve_connection` requires the response body to implement `Body + 'static`; it does not require the body itself to be `Sync`.
- `http-body-util::BodyExt::boxed()` requires `Send + Sync + 'static` and returns `BoxBody`.
- `BodyExt::boxed_unsync()` requires only `Send + 'static` and returns `UnsyncBoxBody`.

References:

- https://docs.rs/hyper/latest/hyper/server/conn/http1/struct.Builder.html#method.serve_connection
- https://docs.rs/http-body-util/latest/http_body_util/trait.BodyExt.html#method.boxed
- https://docs.rs/http-body-util/latest/http_body_util/trait.BodyExt.html#method.boxed_unsync

The likely source of the public `Sync` requirement is therefore EggServe's internal body erasure choice, not an HTTP/Hyper necessity. Prove this before changing the API.

### Required spike

Create a focused implementation branch/change that:

1. changes `ResponseStream`'s erased producer from `Send + Sync` to `Send` only;
2. changes `ResponseStream::new` / `with_known_length` bounds accordingly;
3. changes the internal erased HTTP body type from `BoxBody` to `UnsyncBoxBody` where necessary, using `boxed_unsync()`;
4. updates all internal full/file/stream/error response conversions consistently;
5. compiles the full TCP/TLS/caller-owned-stream runtime without adding synchronization wrappers merely to satisfy type bounds.

If another genuine runtime invariant requires `Sync`, stop and document the exact call chain/type bound. Do not retain `Sync` because `boxed()` happens to require it.

### Regression consumer proving the relaxed contract

Add a public-API compile/runtime test using a producer that is `Send` but intentionally `!Sync`.

A suitable test producer may own `std::cell::Cell` state (or another clearly `Send + !Sync` state type) and implement `Stream<Item = Result<Bytes, ResponseStreamError>>`.

The test must prove:

- the producer is accepted by `ResponseStream`;
- known- and/or unknown-length streaming works over the canonical runtime;
- no unsafe code or `Mutex`/`RwLock` wrapper is required merely to satisfy the API;
- existing `Send + Sync` producers remain source-compatible.

Add an external-consumer compile smoke if the existing public API consumer harness can express the `!Sync` producer cleanly.

### Thread/task safety review

Explicitly verify:

- `Response` and the erased transport body remain `Send` where connection tasks require it;
- no body value is concurrently polled from multiple tasks;
- Python's bounded channel-backed stream adapter still satisfies the relaxed contract;
- client disconnect, shutdown, known-length mismatch, panic/failure, and write-timeout paths remain unchanged;
- TLS and caller-owned transports share the same body type and behavior.

Do not make the entire `Service` or runtime single-threaded to remove `Sync`.

## Track C — API and documentation polish

If the bound is relaxed:

- update rustdoc for `ResponseStream` to state the actual requirement (`Send`, one-shot, single-consumer polling); 
- update `docs/http-primitives.md`, `docs/public-api-boundary.md`, `architecture/runtime.md`, and the streaming example if they imply cross-thread shareability;
- add the relaxed bound to API-stability/public-consumer tests;
- do not expose `UnsyncBoxBody`, Hyper, or http-body-util types publicly.

If the bound cannot be relaxed:

- document the exact reason in the public API docs;
- add a test that pins the required trait bound so it is deliberate rather than accidental;
- keep the internal constraint from leaking additional Hyper types.

## Verification

Run the normal and relevant deep checks after any code change:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test response_streaming
cargo test -p eggserve-core --test transport_driver
cargo test -p eggserve-core --test production_controls
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
```

Also run TLS checks and the installed Python wheel suite because the erased body type crosses both runtime paths.

Do not add an absolute performance gate for this API-polish change. Plan 170 handles representative performance evidence.

## Non-goals

Do not add:

- HTTP/2/3;
- ASGI/WSGI;
- CGI/FastCGI;
- new routing/framework APIs;
- a new response-stream abstraction;
- lock-based wrappers solely to manufacture `Sync`;
- broader privacy or DoS features;
- CI complexity unrelated to the changed contract.

## Acceptance criteria

- [ ] Plans 162–167 have accurate closed/no-go status and implementation SHAs without rewriting historical rationale.
- [ ] Plans 161/168 clearly point their remaining performance-evidence closure to Plan 170.
- [ ] Current-state docs no longer imply that implemented 162–166 work is merely planned.
- [ ] The `ResponseStream` `Sync` requirement has been traced to an actual runtime requirement or removed.
- [ ] If removable, a `Send + !Sync` producer is accepted and works through the canonical HTTP runtime.
- [ ] Hyper/http-body-util erased body types remain internal.
- [ ] Streaming, transport-neutral, production-control, privacy, TLS, examples, and Python wheel regressions remain green.
- [ ] No application-framework/WAF/I2P-protocol responsibilities are added.

## Handoff

Implement this before or in parallel with Plan 170. Plan 170 is the final evidence/claims closure and should update Plans 161 and 168 from “pending performance closure” to their final status.
