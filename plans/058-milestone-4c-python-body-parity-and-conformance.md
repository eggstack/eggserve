# Phase 58 — Milestone 4C: Python Body Parity, Conformance, and Closure

## Goal

Project the bounded Rust request-body contract into Python without reintroducing Python-owned socket I/O, unbounded buffering, duplicate transport logic, or framework-specific semantics. Complete Milestone 4 with cross-language conformance, installed-wheel qualification, release gates, and final-SHA evidence.

This phase must not add async Python handlers, ASGI/WSGI receive channels, multipart/form parsing, upload persistence, request decompression, routing, middleware, or application-framework helpers.

## Starting state

Expected Phase 56–57 outputs:

- canonical request envelope with head, connection metadata, and one-shot body;
- `RequestBodyPolicy` with reject/buffer/stream modes;
- transfer-decoded fixed-length and chunked ingestion;
- hard byte limits;
- body timeout and cancellation;
- partial-consumption drain-or-close policy;
- typed request-body errors;
- static service preservation;
- raw-wire, TLS, fuzz, and resource tests;
- one shared Rust runtime dispatch path.

Python currently has:

- callback services implemented as Rust `Service` adapters;
- synchronous bounded callback execution;
- a canonical request projection;
- installed-wheel lifecycle tests on Linux, macOS, and Windows;
- no async callback support.

## Track A — Python request body surface

Expose a Python body object backed by the Rust `RequestBody` abstraction.

Required conceptual API:

```python
class RequestBody:
    @property
    def declared_length(self) -> int | None: ...

    @property
    def bytes_received(self) -> int: ...

    @property
    def complete(self) -> bool: ...

    def read(self) -> bytes: ...
    def iter_chunks(self, chunk_size: int | None = None) -> Iterator[bytes]: ...
```

Requirements:

- one-shot consumption;
- `read()` and `iter_chunks()` are mutually exclusive;
- no body cloning;
- no implicit rewind;
- no raw transfer framing;
- all limits and timeouts remain Rust-owned;
- Python methods release the GIL while waiting for Rust-owned I/O where technically safe;
- body objects cannot outlive their request/connection semantics in an unsafe way;
- repeated use raises a typed exception;
- empty bodies behave consistently.

Do not expose a file-like API unless its semantics can be implemented exactly and boundedly.

Acceptance:

- Python can consume bounded bodies without owning network reads;
- the API mirrors the Rust conceptual model.

## Track B — Python request projection

Update Python `Request` to include the body object and body metadata while preserving current immutable head fields.

Potential fields:

- `method`;
- `path`;
- `query`;
- duplicate-preserving headers where already supported by the canonical request API;
- `remote_addr`;
- `http_version`;
- `connection_info` if exposed;
- `body`;
- `has_body` as a convenience derived from framing/policy.

Requirements:

- no reparsing of raw HTTP in Python;
- request body comes directly from the Rust request envelope;
- legacy handlers that inspect only metadata continue working;
- body rejection occurs before Python callback invocation when policy is `Reject`;
- request immutability remains intact except for internal one-shot body state;
- `.pyi` stubs describe consumption and exceptions accurately.

Acceptance:

- Python receives the same canonical request semantics as Rust.

## Track C — Python body policy configuration

Define how Python callback servers select body policy.

Preferred constructor options:

```python
Server(
    ...,
    request_body_mode="reject" | "buffer" | "stream",
    max_request_body_bytes=...,
    body_timeout_secs=...,
    incomplete_body_policy="close" | "drain",
)
```

Or a small immutable configuration object if that materially improves correctness.

Requirements:

- default remains `reject` for backward compatibility and safety;
- static mode remains always reject regardless of callback defaults;
- buffer/stream require explicit finite limits;
- configured limit cannot exceed Rust runtime ceiling;
- invalid modes and limits fail at construction;
- drain mode requires explicit byte/time limits;
- constructor defaults are snapshot-tested and cross-checked against docs;
- no hidden unbounded mode.

Acceptance:

- body support is opt-in and bounded;
- existing Python static/callback users remain bodyless unless explicitly configured.

## Track D — Synchronous streaming bridge

For `iter_chunks()`, bridge the async Rust body stream to synchronous Python callbacks safely.

Requirements:

- no per-chunk OS thread creation;
- no nested Tokio runtime creation inside callbacks;
- no deadlock between the Python blocking worker and the runtime task delivering body data;
- bounded channel capacity between async ingestion and synchronous iteration;
- backpressure stops socket reads when Python is slow;
- cancellation wakes blocked Python iteration where feasible;
- handler timeout stops the request task waiting, while underlying Python execution limitations remain truthful;
- callback permit remains accounted for until Python returns;
- shutdown closes the stream and produces a typed body exception;
- chunk size parameter, if accepted, is a presentation buffer size, not a transport-frame guarantee.

Recommended architecture:

- runtime-owned async producer;
- bounded Rust channel;
- Python callback worker consumes channel synchronously;
- producer enforces body byte/time limits;
- channel closure carries terminal success/error.

Acceptance:

- streaming bodies remain bounded in memory and backpressured;
- no transport I/O moves into Python.

## Track E — Python error hierarchy

Expose typed exceptions mapped from `RequestBodyError`, for example:

- `RequestBodyError` base;
- `RequestBodyRejectedError`;
- `RequestBodyTooLargeError`;
- `RequestBodyTimeoutError`;
- `RequestBodyDisconnectedError`;
- `RequestBodyIncompleteError`;
- `RequestBodyConsumedError`;
- `RequestBodyCancelledError`.

Requirements:

- exception inheritance is stable and documented;
- messages are sanitized;
- internal parser/Hyper details are not exposed;
- errors before callback invocation map to HTTP responses rather than Python callback exceptions;
- errors during body consumption are raised from `read()`/iteration;
- callback exception containment remains separate from body exceptions;
- API snapshots verify exports and signatures.

Acceptance:

- Python callers can handle policy, limit, timeout, disconnect, and one-shot failures distinctly.

## Track F — Shared Rust/Python conformance corpus

Extend the normative conformance corpus with request-body cases.

Required fixture groups:

- body policy selection;
- empty body;
- fixed-length exact;
- fixed-length premature EOF;
- fixed-length over-limit;
- conflicting `Content-Length`;
- `Transfer-Encoding` plus `Content-Length` conflict;
- chunked exact;
- chunked many-small-chunks;
- chunked malformed;
- chunked exact-limit and one-over-limit;
- buffer mode;
- stream mode;
- one-shot consumption;
- mixed read/iteration error;
- body timeout;
- client disconnect;
- partial consumption with close policy;
- bounded drain success/failure;
- static-service rejection.

Both Rust and Python runners must consume the same expected semantic outcomes.

Do not encode transport-specific chunk boundaries as normative Python-visible behavior.

Acceptance:

- Rust and Python agree on policy, data, errors, and connection outcomes.

## Track G — Real-wire and external-client tests

Add real-socket tests using raw TCP and standard clients.

Rust/Python scenarios:

- POST/PUT/PATCH extension-method bodies for custom services as supported;
- fixed-length echo/hash service;
- chunked body sent manually;
- slow body;
- disconnect mid-body;
- exact/over-limit;
- body timeout;
- handler reads no body;
- handler reads part then returns;
- second request on same connection after complete body;
- attempted second request after incomplete body;
- TLS equivalents;
- static service body rejection.

External clients:

- Python `http.client` fixed-length request;
- manually chunked client where stdlib permits;
- curl-based smoke only if already acceptable in CI and not required for core correctness.

Acceptance:

- wire behavior matches conformance fixtures and connection policy.

## Track H — Callback timeout and body interaction

Test interactions among:

- handler timeout;
- body timeout;
- callback capacity;
- partial consumption;
- graceful shutdown;
- forced shutdown.

Required invariants:

- whichever configured deadline expires first has deterministic precedence;
- body producer stops when request task is cancelled;
- timed-out Python callback remains counted until it returns;
- body buffers/channels are released after request termination;
- blocked body iterator wakes with a typed terminal error where possible;
- forced shutdown closes listener and runtime tasks even if Python code remains blocked;
- repeated timed-out body callbacks do not create unbounded channels/tasks/threads;
- callback capacity cannot be bypassed by timed-out body work.

Acceptance:

- body support preserves the truthful Python callback timeout model established in Milestone 3.

## Track I — Installed-wheel cross-platform qualification

Extend packaging tests on Linux, macOS, and Windows.

Run outside the source tree with `PYTHONPATH` cleared.

Required installed-wheel cases:

- imports and `.pyi` surface;
- default reject policy;
- buffered body read;
- streamed body iteration;
- exact and over-limit bodies;
- body timeout;
- disconnect handling where platform-stable;
- one-shot errors;
- partial-consumption close behavior;
- callback exception after body read;
- graceful and forced shutdown with active body;
- repeated server cycles;
- no source-tree/native-binary fallback.

Platform-specific handling:

- timing tolerances account for shared runners;
- Windows socket-close behavior is asserted semantically, not by Unix errno;
- all wheels must be native to their runner.

Acceptance:

- every advertised wheel platform has body qualification evidence.

## Track J — Fuzz, property, and resource qualification

Add or complete fuzz/property coverage for:

- Python body bridge state machine;
- Rust/Python one-shot parity;
- bounded channel behavior;
- malformed framing corpus replay;
- decoded-size accounting;
- cancellation interleavings;
- partial-consumption connection decisions.

Resource tests:

- many small buffered requests;
- body at maximum limit;
- slow streaming consumer;
- repeated timeout/disconnect cycles;
- callback pool saturation with active bodies;
- shutdown during ingestion;
- memory/permit/task counts return to baseline.

Acceptance:

- no unbounded memory, task, channel, or worker growth is observed.

## Track K — API stability, migration, and docs

Update:

- `docs/api-stability.md`;
- `docs/release-contract.md`;
- `docs/library-capability-matrix.md`;
- `docs/python-api.md`;
- `architecture/runtime.md`;
- `architecture/eggserve-python.md`;
- migration guide;
- README and examples;
- `.pyi` files;
- AGENTS/skill references.

Required truths:

- request-body APIs remain experimental through Milestone 4 closure;
- default policy is reject;
- built-in static service remains GET/HEAD and bodyless;
- limits apply to decoded bytes;
- raw chunk framing is unavailable;
- callback/body timeouts do not cancel arbitrary Python code;
- partial consumption determines connection reuse;
- no multipart, forms, uploads, decompression, or trailers are implied.

Acceptance:

- contract consistency tests catch default, limit, exception, and policy drift.

## Track L — Dedicated release gates

Add stable criteria and CI evidence for Milestone 4.

Recommended gates:

- `body.public-rust-consumer`;
- `body.fixed-length-accounting`;
- `body.chunked-accounting`;
- `body.limit-enforcement`;
- `body.timeout-cancellation`;
- `body.partial-consumption`;
- `body.static-rejection`;
- `body.tls-parity`;
- `python.body-parity`;
- `python.body-timeout`;
- `python.body-lifecycle-linux`;
- `python.body-lifecycle-macos`;
- `python.body-lifecycle-windows`;
- `body.corpus-replay`;
- `body.fuzz-smoke` where appropriate.

Each gate must define:

- trigger policy;
- command;
- platform applicability;
- invalidation paths;
- evidence class;
- freshness;
- required artifacts;
- release-required status.

Use distinct structured evidence records. Add negative aggregation tests for missing and wrong-SHA body evidence.

Acceptance:

- Milestone 4 guarantees are visible in the machine-readable release model.

## Track M — Final-SHA qualification

Select one clean main-branch candidate after all Milestone 4 work.

Run and inspect:

- Rust body unit/integration/raw-wire/TLS tests;
- shared conformance corpus in Rust and Python;
- Python source tests;
- Linux/macOS/Windows installed-wheel tests;
- fuzz/corpus replay;
- resource qualification;
- package and supply-chain gates;
- evidence aggregation.

Download evidence and regenerate the checklist locally.

Mutation checks:

- remove body gate evidence;
- alter SHA;
- alter wheel digest;
- introduce conflicting pass/fail record;
- mark over-limit test skipped.

All must fail closed.

Record:

- candidate SHA;
- run IDs;
- artifact names/digests;
- platform results;
- aggregate/checklist digest;
- known limitations.

## Required validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo test -p eggserve-core --test request_body_integration
cargo test -p eggserve-core --test request_body_wire
cargo test -p eggserve-core --features tls --test request_body_tls
cargo test -p eggserve-core --test public_api_consumers
python3 -m unittest scripts.test_release_criteria -v
python3 -m unittest scripts.test_check_contract_consistency -v
python3 -m unittest scripts.test_release_safety -v
python3 scripts/check-contract-consistency.py
python3 scripts/release_criteria.py validate release/criteria.toml
python3 scripts/release_criteria.py generate-checklist --check
bash scripts/release-validate.sh full
```

Run the complete installed-wheel matrix on Linux, macOS, and Windows.

## Completion criteria

Milestone 4 is complete only when:

- Rust services can reject, buffer, or stream bounded transfer-decoded bodies;
- fixed-length and chunked accounting enforce the same decoded-byte ceiling;
- body timeout, cancellation, and disconnect behavior are deterministic;
- one-shot consumption is enforced in Rust and Python;
- partial consumption has deterministic drain-or-close semantics;
- static serving remains bodyless;
- Python body APIs use Rust-owned I/O and bounded backpressure;
- callback timeout/resource semantics remain truthful;
- Rust/Python share conformance fixtures;
- Linux, macOS, and Windows installed wheels pass body qualification;
- dedicated body evidence gates pass on one exact SHA;
- downloaded evidence regenerates the same checklist;
- docs, stubs, capability matrix, and release contract match behavior.

## Non-goals

- Multipart/form parsing.
- Upload storage or temporary-file management.
- Automatic JSON/form helpers.
- Content decompression.
- HTTP trailers.
- Async Python callbacks.
- ASGI/WSGI receive semantics.
- Routing or middleware.