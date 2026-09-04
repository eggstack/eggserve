# Plan 171 — Release API and Qualification Metadata Polish

## Status

**IMPLEMENTED / CLOSED.**

## Closure record

Track A accepted the explicit pre-1.0 breaking-release outcome. The current
`main` API keeps the one-owner `ResponseStream` producer model (`Send` without
`Sync`) and leaves the public `to_hyper_response()` body type opaque; callers
must use inference or the `http_body::Body` contract. The former named
`BoxBody` return shape and the semaphore-aware conversion helper are not
preserved as stable downstream contracts. The next release containing this
transition is classified as `0.2.0`, with migration/release-note guidance in
`docs/migration-guide.md`.

Track B corrected the Plan 170 Python profile metadata without changing any
measured values. The harness now emits the measured callback variants as
`[8, 16]`, and the tracked evidence records the same correction.

Track C enumerated the two intentional public Hyper conversion adapters
(`RequestHead::try_from_hyper()` inbound and `to_hyper_response()` outbound)
while retaining a Hyper-free canonical/Service/caller-owned API boundary.

Prerequisite: Plans 161–170 are implemented and closed. This plan is a narrow release-polish pass only; it must not reopen the production runtime, response-streaming ownership model, privacy policy, Python substrate, or qualification architecture.

## Goal

Resolve the three remaining release-hygiene issues found after Plans 169–170:

1. make the stable/public contract around `primitives::to_hyper_response()` consistent with the Plan 169 `Send + !Sync` response-stream implementation and with EggServe's pre-1.0 versioning policy;
2. correct the Plan 170 Python benchmark profile metadata so it accurately describes the measured 8- and 16-callback variants; and
3. reconcile the stated "no Hyper in public API" invariant with the intentionally public low-level Hyper conversion boundary.

The desired outcome is a release-ready tree whose code, stability inventory, migration guidance, tests, and benchmark evidence all describe the same contract.

## Current state

### Plan 169 body-erasure change

Plan 169 correctly relaxed the stable `ResponseStream` producer requirement from `Send + Sync` to `Send` and changed the internal erased response body from `http_body_util::combinators::BoxBody` / `.boxed()` to `UnsyncBoxBody` / `.boxed_unsync()`.

This was necessary to support ordinary one-owner `Send + !Sync` producers without adding synchronization wrappers. A runtime regression test using `Cell` proves the relaxed producer contract over the real server path.

The ownership model is now intentional:

- `ResponseStream` is one-shot;
- one connection task owns and polls it;
- the producer must be `Send + 'static`;
- the producer does not need to be `Sync`;
- concurrent body polling is unsupported;
- TCP, TLS, and caller-owned transports share this model.

Do not reverse this change merely to recover an old erased-body type.

### Public conversion signature

Before Plan 169, stable `primitives::to_hyper_response()` returned a concrete:

```rust
hyper::Response<http_body_util::combinators::BoxBody<bytes::Bytes, std::io::Error>>
```

After Plan 169 it returns:

```rust
hyper::Response<impl http_body::Body<Data = bytes::Bytes, Error = std::io::Error>>
```

and the semaphore-aware conversion helper was made crate-private.

The new signature is a better abstraction because EggServe no longer promises a specific erased body implementation. However, changing a public stable function from a named concrete return type to opaque `impl Trait` can break downstream source code that names, stores, constrains, or otherwise depends on the former return type.

The repository currently remains version `0.1.2`. The stability documents also contain slightly different wording about what breaking changes mean before 1.0. This must be resolved before publishing this state as a release.

### Plan 170 Python benchmark metadata

`benchmarks/170-closure/results.json` contains real Python low-level workload records for both:

- `max_python_callbacks = 8`; and
- `max_python_callbacks = 16`.

Individual workload records correctly identify their callback limit, but the top-level `profiles.python_lowlevel.runtime_limits` currently records only `max_python_callbacks: 8`.

This is evidence-metadata drift, not a benchmark correctness failure.

### Hyper-boundary wording

`crates/eggserve-core/tests/no_hyper_in_public_api.rs` currently says the only intentional public Hyper exception is `RequestHead::try_from_hyper`.

That is no longer a complete description of the contract: `primitives::to_hyper_response()` is also intentionally public and is documented elsewhere as an explicit low-level transport conversion boundary.

The product invariant should be stated more precisely: application-facing canonical types and the server/service embedding boundary are Hyper-free; narrowly identified conversion adapters may intentionally mention Hyper.

## Track A — Resolve `to_hyper_response()` release compatibility

### A1. Treat Plan 169 ownership as fixed

The following are not negotiable for compatibility polish:

- do not restore `ResponseStream: Sync`;
- do not wrap producers in `Mutex`, `RwLock`, or another synchronization layer solely to satisfy `BoxBody`;
- do not expose `UnsyncBoxBody` as a new stable application-facing abstraction merely to preserve a concrete return type;
- do not add a second parallel response-stream implementation;
- do not leak additional Hyper/http-body-util internals into `Service`, `Response`, or `ResponseStream`.

A compatibility solution is acceptable only if it preserves the one-owner `Send` body model.

### A2. Perform a focused source-compatibility check

Create a temporary external Rust consumer pinned to the last published/intended pre-Plan-169 API shape and exercise realistic uses of `to_hyper_response()`:

1. call it with type inference only;
2. explicitly name the previous `Response<BoxBody<...>>` return type;
3. store the result in a helper returning that named type;
4. pass the body to a generic function requiring `http_body::Body` but not a concrete erased type.

Compile the consumer against:

- the baseline release/API state; and
- current `main`.

Record which patterns are actually source-breaking. Do not infer compatibility solely from API-diff text.

### A3. Preferred compatibility outcome

If there is a clean EggServe-owned compatibility adapter that:

- preserves existing source compatibility for callers that did not require `Sync` body sharing;
- keeps `ResponseStream` producers `Send + !Sync` capable;
- does not expose a new third-party erased-body type as a stable contract; and
- does not duplicate the transport pipeline,

implement it and add external-consumer coverage.

Examples of potentially acceptable approaches include a small EggServe-owned transport body wrapper/newtype whose public semantic contract is only `Body<Data = Bytes, Error = io::Error> + Send`, provided it does not force producer synchronization or expose internal implementation choices.

Do not introduce such a wrapper if it is more API machinery than the compatibility benefit justifies.

### A4. Explicit breaking-release outcome

If preserving the old concrete return type would require reintroducing `Sync`, leaking `UnsyncBoxBody`, or adding disproportionate compatibility machinery, accept the signature change as an intentional pre-1.0 breaking API transition.

In that case:

- do not publish it as a patch-level `0.1.x` compatibility-preserving change;
- select the next version according to the repository's documented pre-1.0 policy, with `0.2.0` as the expected SemVer-style breaking transition from `0.1.x` unless the project's release policy explicitly establishes another scheme;
- add a concise migration note explaining that `to_hyper_response()` no longer promises `BoxBody` and that downstream code should depend on the `Body` behavior rather than a concrete erasure type;
- document the removal/internalization of `to_hyper_response_with_file_stream_semaphore()` if it was previously part of the supported stable surface;
- update API snapshots/stability tests so the new contract is deliberate;
- make release notes identify the change before publishing.

Do not bump the package version as part of this plan unless the repository's normal release convention explicitly performs version selection during implementation-plan closure. It is sufficient to make the required next-release classification unambiguous for handoff.

### A5. Reconcile versioning-policy wording

Audit at minimum:

- `docs/api-stability.md`;
- `docs/public-api-boundary.md`;
- `docs/release-contract.md`;
- `docs/release-process.md`;
- `docs/migration-guide.md`.

There must be one consistent pre-1.0 rule for stable Rust API changes.

Recommended rule:

> Patch releases preserve stable source compatibility. Before 1.0, intentional breaking changes to stable Rust APIs require an explicit minor-version transition (for example `0.1.x` → `0.2.0`), release notes, and migration guidance. Experimental APIs may change under their separately documented policy.

If the repository already has a stronger rule, retain the stronger rule rather than weakening it for this change.

## Track B — Correct Plan 170 Python benchmark metadata

Update the benchmark harness and tracked evidence representation so the top-level Python profile cannot imply one callback limit when multiple variants were measured.

Preferred representation:

```json
"python_lowlevel": {
  "runtime_limits": {
    "max_connections": 128,
    "max_in_flight_requests": 128
  },
  "measured_max_python_callbacks": [8, 16]
}
```

or an equivalently unambiguous variant/profile structure.

Requirements:

- preserve the existing per-workload `max_python_callbacks` values;
- do not alter timing/RPS/latency numbers merely to fix metadata;
- if `results.json` is edited directly, add a note identifying it as a metadata-only correction and retain the original capture source SHA/timestamp semantics;
- preferably make `benchmark.py` generate the corrected structure so future captures cannot recreate the inconsistency;
- validate the JSON with the repository's existing evidence/check scripts or a small deterministic parser check;
- do not rerun the entire benchmark matrix unless the harness cannot be corrected without doing so.

If a new capture is performed for another reason, preserve the old capture rather than silently replacing historical evidence.

## Track C — Define the public Hyper boundary precisely

### C1. Correct the test/module invariant

Update `crates/eggserve-core/tests/no_hyper_in_public_api.rs` and related comments so they no longer claim that `RequestHead::try_from_hyper` is the only public Hyper exception.

The intended invariant is:

> Canonical application-facing request/response types, `Service`, and the caller-owned connection API do not require downstream code to import Hyper. Explicit conversion adapters at the transport boundary may mention Hyper and are individually documented.

At minimum identify:

- `RequestHead::try_from_hyper` as an inbound conversion adapter; and
- `to_hyper_response()` as an outbound low-level conversion adapter if Track A retains it as public.

If Track A instead deprecates/internalizes the outbound adapter under an explicit breaking transition, make the test describe that final state.

### C2. Mechanical public-boundary coverage

Improve the test so it verifies behavior rather than relying only on prose where practical.

Examples:

- canonical `Response`, `ResponseBody`, and `ResponseStream` can be constructed without naming Hyper;
- `Service` / `serve_http1_connection` consumer examples do not require Hyper;
- intentional conversion functions are isolated and enumerated in one test/comment table.

Do not attempt brittle source scanning for the string `hyper` across the whole crate; internal Hyper use is expected.

### C3. Documentation consistency

Audit references in:

- `README.md`;
- `docs/public-api-boundary.md`;
- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `architecture/primitives-api.md`;
- `architecture/runtime.md`;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`.

Use one distinction consistently:

- **application-facing/canonical boundary:** Hyper-free;
- **explicit transport conversion adapters:** may mention Hyper;
- **internal runtime implementation:** Hyper is an implementation dependency.

Do not weaken the product statement into "Hyper-free everywhere" and do not imply downstream application servers need Hyper.

## Verification

For any Rust API change or compatibility adapter, run at minimum:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test no_hyper_in_public_api
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test response_streaming
cargo test -p eggserve-core --test transport_driver
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
```

Also run:

- TLS lint/tests because the body erasure crosses TLS responses;
- `bash scripts/verify-cargo-packages.sh --mode all`;
- the external-consumer compatibility smoke from Track A;
- the installed Python wheel suite if any shared native response type changes;
- a deterministic parse/validation of `benchmarks/170-closure/results.json` after metadata correction.

Routine CI should remain small. Do not add benchmark timing gates or a new always-on compatibility matrix for this polish pass.

## Acceptance criteria

Plan 171 is complete when all of the following are true:

- [x] the repository has an explicit, technically justified compatibility decision for the Plan 169 `to_hyper_response()` signature change;
- [x] that decision does not reintroduce `Sync` as a `ResponseStream` producer requirement;
- [x] if source compatibility cannot be preserved cleanly, the next release is explicitly classified as a pre-1.0 breaking transition rather than a patch-level compatible release;
- [x] migration/release documentation names any affected stable conversion helper and the replacement contract;
- [x] the pre-1.0 stable-API versioning rule is stated consistently across authoritative docs;
- [x] external-consumer tests cover the chosen transport-conversion contract;
- [x] Plan 170 Python benchmark metadata accurately represents both callback-limit variants without changing measured numbers;
- [x] the benchmark harness emits the corrected metadata on future runs;
- [x] `no_hyper_in_public_api.rs` and current-state docs accurately enumerate intentional Hyper conversion boundaries;
- [x] canonical `Service`/response/connection consumers still require no direct Hyper dependency;
- [x] Rust/TLS/Python/package verification remains green;
- [x] no production runtime, security, privacy, I2P, CGI/FastCGI, ASGI/WSGI, or benchmark-architecture work is reopened.

## Non-goals

Do not add:

- new HTTP features;
- HTTP/2 or HTTP/3;
- new response-body synchronization;
- a second streaming abstraction;
- framework/router/middleware APIs;
- application-server process management;
- new performance claims or benchmark workloads;
- an arm64 performance requirement;
- release automation or expanded routine CI;
- unrelated API cleanup.

## Handoff

Implement this as one narrow release-polish change set.

Start with Track A because its compatibility outcome determines the exact documentation and tests required by Track C. Then correct the Plan 170 metadata in Track B, reconcile the Hyper-boundary wording/tests in Track C, and finish with the external-consumer plus package/TLS/Python verification pass.

If Track A concludes that preserving the old concrete `BoxBody` signature conflicts with the validated `Send + !Sync` stream model, prefer the stream model and classify/document the release break explicitly. Do not sacrifice the corrected runtime ownership architecture for cosmetic source compatibility.
