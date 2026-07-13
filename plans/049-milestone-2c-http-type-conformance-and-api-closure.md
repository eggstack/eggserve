# Phase 49 — Milestone 2C: HTTP Type Conformance, Cross-Language Parity, and API Closure

## Goal

Validate and close the canonical request and response work from Phases 47–48. This phase must prove that Rust, Python, raw-wire behavior, static serving, and downstream public API use all agree on one contract before the new HTTP value types are promoted to stable.

This is a conformance and closure phase, not a feature phase.

## Starting state

After Phases 47–48, eggserve should have:

- transport-independent request and response types;
- validated methods, versions, headers, status codes, and targets;
- duplicate-preserving headers in Rust and Python;
- connection metadata;
- one final response normalization path;
- runtime-owned framing and HEAD behavior;
- static-response adapters;
- typed construction and normalization errors.

The remaining work is to detect semantic drift, migration gaps, performance regressions, incomplete Python parity, or accidental public dependency leakage.

## Track A — Normative conformance corpus

Create a versioned corpus of language-neutral fixtures, preferably JSON or another simple checked-in format, describing:

- input request wire/head data;
- expected canonical request representation;
- response-builder input;
- expected normalized status/headers/body disposition;
- expected error type for invalid cases;
- applicability by HTTP version and method.

Corpus groups must include:

- methods and extension methods;
- request-target forms;
- duplicate headers;
- invalid names/values;
- HTTP/1.0 and HTTP/1.1;
- status/body compatibility;
- HEAD normalization;
- Content-Length and Transfer-Encoding conflicts;
- hop-by-hop fields;
- static conditional and range responses;
- file and stream body metadata;
- connection metadata projection.

Fixtures must avoid embedding implementation details. They define observable contract behavior.

## Track B — Rust conformance runner

Add a Rust test harness that loads every fixture and exercises only public or deliberately test-exposed contract entry points.

Requirements:

- deterministic fixture ordering;
- fixture IDs in failures;
- exact header-order and duplicate assertions where contractual;
- exact typed-error assertions;
- raw-wire verification for normalized output;
- no hidden use of internal representations to make tests pass;
- corpus schema validation.

Add property tests for invariants not practical to enumerate:

- accepted method/header values round-trip;
- normalization is idempotent;
- normalized responses never contain conflicting framing;
- HEAD never emits body bytes;
- body-forbidden statuses never emit payload bodies;
- duplicate end-to-end fields remain ordered;
- malformed input never reaches a handler.

## Track C — Python parity runner

Run the same corpus against the installed Python wheel.

Requirements:

- no source-tree imports;
- canonical ordered headers;
- same status/method/version values;
- same request target projection;
- same error categories;
- same normalization outcomes;
- immutable public request values;
- file responses remain Rust-owned and streaming;
- callback boundary behavior tested over real sockets.

Where Python ergonomics require different object shapes, document the mapping while preserving semantics. Do not accept semantic divergence merely because the language surfaces differ.

## Track D — Raw-wire and interoperability validation

Expand raw-wire tests around the new canonical types and normalization path.

Test with independent clients where practical:

- curl;
- Python `http.client`;
- Python `requests` when available in an isolated optional job;
- Rust client fixture;
- Node fetch if already supported in CI or kept as an optional interoperability script.

Validate:

- exact response framing;
- keep-alive/close behavior;
- duplicate response headers;
- HEAD;
- ranges and conditionals;
- malformed requests;
- HTTP/1.0 versus HTTP/1.1;
- callback-generated responses;
- large file responses.

Do not add mandatory heavy dependencies to the core crate for interoperability tests.

## Track E — Public API compile and import fixtures

Create external-consumer fixtures outside the crate module tree.

Rust fixtures must compile examples for:

- constructing methods and headers;
- inspecting `RequestHead`;
- accessing connection metadata;
- building byte, empty, file, and stream responses;
- handling typed errors;
- using public APIs without importing Hyper.

Add checks such as `cargo-semver-checks` if compatible with project policy, or maintain explicit API snapshots and compile fixtures.

Python fixtures must verify:

- `__all__`;
- constructor signatures;
- type stubs;
- exception hierarchy;
- ordered header APIs;
- request immutability;
- wheel-only imports;
- no accidental internal exports.

## Track F — Migration and compatibility closure

Inventory superseded APIs and decide for each:

- retain as stable adapter;
- deprecate with warning/documentation;
- keep experimental;
- remove before release because it was never stable;
- retain internally only.

Likely areas:

- flat Python header dictionaries;
- legacy request method strings;
- old response constructors;
- direct static response plan conversion;
- duplicate request model types;
- internal Hyper-facing types accidentally public.

Provide migration examples. Avoid maintaining two full canonical representations indefinitely.

## Track G — API stability review

Perform an explicit review before promotion.

For each new exported item record:

- stability tier;
- construction invariants;
- exhaustiveness policy;
- equality/hash behavior;
- string/display guarantees;
- thread-safety/send-sync properties;
- Python mutability and pickling policy;
- error stability;
- ordering guarantees;
- pre-1.0 breaking-change policy.

Promote to stable only when:

- corpus coverage exists;
- Rust/Python parity passes;
- compile/import fixtures pass;
- docs are complete;
- no known semantic ambiguity remains.

Otherwise leave the item experimental and state the blocker.

## Track H — Performance and allocation regression review

Benchmark representative paths before and after Milestone 2:

- simple GET request parse/dispatch;
- HEAD response;
- duplicate-header request;
- static small file;
- static large file;
- range response;
- Python callback response;
- response normalization;
- connection metadata construction.

Measure:

- allocations per request;
- request latency;
- throughput under bounded concurrency;
- memory behavior;
- file streaming throughput;
- Python callback overhead.

The goal is not premature optimization. Any material regression must be understood and either corrected or documented. Canonical types must not accidentally force large file bodies through memory.

## Track I — Fuzzing expansion

Add or update fuzz targets for:

- method construction;
- header-name/value validation;
- header block lookup and duplicate handling;
- request-head conversion;
- response builder;
- normalization;
- content-length reconciliation;
- status/body rules.

Seed with normative and adversarial corpus cases. Crashes, panics, non-idempotent normalization, or invalid normalized framing are failures.

Ensure corpus replay runs in normal CI and new fuzz targets are included in scheduled/manual fuzz workflows.

## Track J — Documentation and examples

Update all affected documentation:

- README library examples;
- Rust crate docs;
- Python API reference;
- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- architecture request/response/server docs;
- migration guide;
- non-goals;
- release criteria and gate inventory.

Add examples for:

- duplicate request headers;
- safe unique-header access;
- connection metadata;
- correct HEAD handling without handler special-casing;
- duplicate response headers;
- file-backed response;
- invalid framing rejection.

## Track K — Release-gate integration

Add stable criteria gates for Milestone 2 conformance:

- canonical HTTP corpus Rust;
- canonical HTTP corpus Python;
- public Rust compile fixtures;
- Python API/import fixtures;
- raw-wire normalization;
- fuzz corpus replay;
- generated API inventory;
- performance regression report if maintained as advisory evidence.

Use the corrected Milestone 1 evidence system. Every gate must emit exact-SHA structured evidence.

## Required validation

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test http_primitives_integration
cargo test -p eggserve-bin --test production_path
cargo test -p eggserve-core --test corpus_replay --features client
python3 -m unittest scripts.test_release_criteria -v
python3 scripts/check-contract-consistency.py
bash scripts/release-validate.sh fast
```

Also run all new Rust/Python conformance, compile/import, installed-wheel, fuzz replay, and benchmark comparison commands documented by the implementation.

## Completion criteria

Milestone 2 is complete only when:

- one normative corpus is executed by Rust and installed Python;
- raw-wire output confirms normalization semantics;
- duplicate headers are preserved across all boundaries;
- public consumer fixtures compile/import without internal or Hyper dependencies;
- superseded APIs have explicit migration/deprecation outcomes;
- all new exports have reviewed stability classifications;
- fuzz/property tests cover construction and normalization invariants;
- static-server behavior remains compatible;
- no unexplained material performance regression remains;
- release criteria and evidence gates cover the new contract.

## Non-goals

- Request body streaming; that belongs to Milestone 4.
- General server runtime redesign; that belongs to Milestone 3.
- Routing, middleware, ASGI, or WSGI.
- HTTP/2, WebSockets, or trailers.
- Client feature expansion beyond tests needed for interoperability.