# Phase 50 — Milestone 2 Final Closure

## Goal

Fully close Milestone 2 by reconciling the remaining response-model, protocol-correctness, API-stability, benchmark, and release-evidence gaps in Plans 047–049.

The canonical request model, canonical response model, Python projections, conformance corpus, fuzz targets, benchmarks, and CI gates already exist. This phase is a focused correctness and integration pass. It must not broaden scope into request-body streaming, routing, middleware, ASGI/WSGI, HTTP/2, or general framework behavior.

## Starting state

The repository now has:

- canonical request types: `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, and `ConnectionInfo`;
- canonical response types: `StatusCode`, `ResponseHead`, `ResponseBody`, `Response`, and `ResponseBuilder`;
- response normalization for HEAD, body-forbidden statuses, hop-by-hop headers, and content length;
- Rust and Python bindings with typing stubs;
- a shared cross-language conformance corpus;
- raw-wire and external-client tests;
- public-consumer fixtures;
- canonical-type fuzz targets and seed corpora;
- Criterion and allocation-oriented benchmarks;
- CI and release-gate wiring.

Remaining concerns:

1. `StatusCode` accepts values below 100 even though HTTP status codes are three digits;
2. file-backed and range responses retain a separate transport path and do not fully participate in the canonical response model;
3. documentation claims a single normalization path more broadly than implementation currently proves;
4. singleton and framing-header semantics need explicit enforcement at canonical conversion boundaries;
5. request/response stability labels and migration rules are not fully consistent;
6. benchmark execution is too tightly coupled to the ordinary multi-platform PR matrix;
7. a real final-SHA Milestone 2 evidence bundle has not been inspected and recorded.

## Track A — Correct status-code validation

Change canonical `StatusCode` validation so invalid one- and two-digit values cannot be constructed.

Decide and document one range policy:

- preferred: `100..=999`, allowing extension codes while preserving three-digit syntax; or
- stricter: `100..=599`, if eggserve deliberately rejects nonstandard extension classes.

The selected policy must be consistent across:

- Rust constructors and `TryFrom` implementations;
- Python constructors;
- Hyper conversion;
- response builders;
- conformance corpus;
- fuzz targets;
- API documentation;
- error text and exception taxonomy.

Add explicit tests for:

- 0, 1, 42, 99;
- 100, 199, 200, 204, 304, 599;
- 600 and 999 according to the chosen extension policy;
- 1000 and larger;
- round-trip Hyper conversion;
- Python parity.

Acceptance:

- every constructible `StatusCode` can be serialized as a valid three-digit HTTP status;
- transport conversion cannot fail merely because a previously accepted status was syntactically invalid;
- corpus and fuzz expectations match the selected policy.

## Track B — Unify canonical response metadata with file and range bodies

The canonical response layer must become authoritative for response metadata and framing across all response producers without copying file contents into memory.

Preferred architecture:

```rust
pub enum ResponseBody {
    Empty,
    Bytes(Bytes),
    File(BodySource),
}
```

or an equivalent split:

```rust
pub struct NormalizedResponse {
    head: NormalizedResponseHead,
    body: TransportBody,
}

pub enum TransportBody {
    Empty,
    Bytes(Bytes),
    FileFull(BodySource),
    FileRange(BodySource),
}
```

Requirements:

- preserve Rust-owned file handles and streaming;
- preserve range metadata and exact byte counts;
- preserve file-stream semaphore ownership;
- do not eagerly read file bodies;
- apply HEAD suppression before transport transmission while retaining representation headers;
- suppress bodies for 1xx, 204, and 304 across byte, file, and range bodies;
- reconcile `Content-Length` from authoritative body metadata;
- strip or reject handler-owned framing headers consistently;
- preserve stream I/O error propagation;
- keep one-shot body-consumption semantics.

Define how existing `BodySource`, `BodyPlan`, and `StaticResponsePlan` convert into the canonical model. Avoid duplicating file-body enums if existing capability types can be reused safely.

Acceptance:

- static full-file, range, directory/error, and Python callback responses all pass through one normative response-head/framing policy;
- file and range responses remain streaming and capability-backed;
- no extra Python or Rust memory copy is introduced;
- HEAD and body-forbidden behavior is identical for bytes and files.

## Track C — Audit and eliminate parallel normalization logic

Perform a repository-wide audit for direct response construction and framing decisions.

Search for all paths that:

- construct `hyper::Response` directly;
- set or remove `Content-Length`;
- set or remove `Transfer-Encoding`;
- handle HEAD;
- handle 1xx, 204, or 304;
- construct static errors;
- convert `StaticResponsePlan`;
- convert Python callback results;
- emit full-file or range responses;
- create transport bodies.

Classify each site as:

- canonical producer;
- canonical adapter;
- transport conversion;
- legacy path to remove;
- test-only helper.

Add a small architecture document or table identifying the one allowed sequence:

```text
producer -> canonical response -> normalize -> transport conversion
```

Where file transport requires a separate body carrier, document that only the body transport differs; response metadata and framing policy must remain canonical.

Extend contract-consistency or source-boundary tests where practical so newly introduced direct framing paths fail CI.

Acceptance:

- no production response bypasses canonical metadata normalization;
- duplicate HEAD/body-forbidden/framing policy implementations are removed;
- documentation accurately describes the actual architecture.

## Track D — Tighten header cardinality and framing semantics

`HeaderBlock` should preserve duplicates, but canonical request and response conversion must enforce field-specific invariants.

Define explicit policy for at least:

- `Host`;
- `Content-Length`;
- `Transfer-Encoding`;
- `Connection`;
- `Trailer`;
- `Upgrade`;
- `TE`;
- `Set-Cookie`;
- general comma-list fields.

Request-side requirements:

- duplicate or conflicting `Content-Length` cannot reach handlers;
- `Content-Length` plus `Transfer-Encoding` cannot reach handlers;
- invalid duplicate `Host` is rejected according to the HTTP/1 contract;
- framing fields are validated before constructing canonical `RequestHead`;
- malformed singleton fields produce typed conversion errors.

Response-side requirements:

- runtime-owned framing headers cannot be supplied ambiguously by callbacks;
- duplicate `Set-Cookie` and other non-combinable end-to-end fields remain preserved;
- no implicit comma-joining is performed unless explicitly requested by an API designed for a combinable field;
- `get_unique()` ambiguity remains typed and deterministic.

Add corpus cases, raw-wire tests, Python parity tests, and fuzz assertions.

Acceptance:

- duplicate preservation does not weaken framing correctness;
- malformed singleton/framing fields never reach application callbacks;
- response normalization owns final framing.

## Track E — Resolve stability and migration policy

Reconcile all public documentation and code comments so canonical APIs have one status.

Preferred policy:

- canonical request and response types remain public and experimental for the first release containing them;
- existing stable types remain supported;
- migration helpers are documented;
- no experimental type is described as stable merely because it is publicly exported;
- promotion requires completed conformance, one release cycle, and no unresolved response-path gaps.

Review paired old/new types:

- `ReadOnlyMethod` / `Method`;
- `ResponseStatus` / `StatusCode`;
- `HeaderMapPlan` / `HeaderBlock`;
- `StaticResponsePlan` / canonical `Response`.

For each pair, document one of:

- long-term distinct roles;
- deprecation path;
- conversion bridge;
- planned replacement milestone.

Update:

- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/migration-guide.md`;
- `docs/library-capability-matrix.md`;
- Rust docs;
- Python docs and stubs;
- API-stability tests.

Acceptance:

- stable/experimental labels are consistent everywhere;
- downstream consumers have an explicit migration story;
- no accidental stability promise is created.

## Track F — Rationalize benchmark and allocation gates

Separate correctness of benchmark targets from actual performance qualification.

Recommended CI policy:

- PR CI: compile benchmarks and run Criterion `--test` on one bounded Linux job;
- main/scheduled/manual performance job: execute real benchmarks and upload reports;
- allocation tests: use a focused deterministic threshold or regression report rather than relying only on ad hoc `dhat` output;
- macOS/Windows PR matrices should not repeatedly compile and run the full Criterion suite unless required for portability.

Record:

- baseline commit;
- benchmark environment;
- median/variance;
- allocation counts for representative operations;
- allowed regression threshold;
- advisory versus release-blocking status.

Keep `dhat` only if its output is consumed by a reproducible test or report. Otherwise remove unnecessary dependency weight.

Acceptance:

- ordinary PR latency is bounded;
- benchmark targets remain compile-tested;
- real performance evidence is generated in a suitable environment;
- allocation instrumentation has a defined purpose and pass criterion.

## Track G — Final conformance and runtime-path tests

Add end-to-end tests covering the corrected architecture:

- GET byte body;
- HEAD byte body;
- GET full file;
- HEAD full file;
- GET range;
- HEAD range;
- 204 with byte/file body supplied;
- 304 with byte/file body supplied;
- callback-supplied `Content-Length` mismatch;
- callback-supplied `Transfer-Encoding`;
- duplicate `Set-Cookie` preservation;
- file stream I/O error after headers;
- malformed request framing never reaches handler;
- canonical normalization idempotency across all body kinds;
- Rust/Python parity.

Use real sockets for representative cases and direct canonical tests for exhaustive cases.

Acceptance:

- the same semantic fixtures pass in Rust and Python;
- raw wire output confirms final framing;
- file streaming remains Rust-owned.

## Track H — Release evidence and final-SHA verification

Wire any new tests and benchmark-policy changes into `release/criteria.toml` and CI.

Run a clean main-branch workflow on the final Milestone 2 closure SHA and inspect:

- canonical conformance gates;
- raw-wire interoperability gate;
- public consumer fixtures;
- Python installed-wheel tests;
- benchmark compile/performance artifacts;
- fuzz target matrix and corpus replay;
- package gates;
- evidence aggregation;
- exact-SHA checklist generation.

Record workflow run IDs and artifact digests through the release evidence system. Do not treat commit-message test counts as release evidence.

Acceptance:

- one final-SHA evidence bundle proves Milestone 2 closure;
- no required gate is missing, stale, malformed, or invalidated;
- downloaded evidence deterministically regenerates the same checklist.

## Required validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test canonical_conformance
cargo test -p eggserve-core --test canonical_wire_interop
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test no_hyper_in_public_api
cargo bench -p eggserve-core --bench canonical_types -- --test
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_check_contract_consistency -v
python3 -m unittest scripts.test_release_safety -v
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
bash scripts/release-validate.sh metadata
```

Build an installed wheel and run canonical request, response, conformance, external-client, API-consumer, and file-streaming tests with `PYTHONPATH` unset.

## Completion criteria

Milestone 2 is fully closed only when:

- `StatusCode` accepts only valid three-digit values under a documented policy;
- file and range responses share canonical response metadata normalization;
- no production response bypasses the normative framing path;
- singleton and framing headers are enforced correctly;
- API stability labels and migration rules are consistent;
- benchmark CI is maintainable and evidence-producing;
- Rust/Python conformance remains green;
- a final-SHA evidence bundle has been inspected and recorded;
- no Milestone 2 correctness or contract blocker remains.

## Non-goals

- Request-body streaming.
- Routing, middleware, or framework APIs.
- HTTP/2 or WebSockets.
- ASGI/WSGI adapters.
- Proxy-header trust.
- General-purpose streaming producers beyond the body carriers already needed for file serving.