# Plan 087 — Structured Logging and Operational Error Closure

## Goal

Make eggserve’s operational behavior truthful, machine-consumable, bounded, and diagnosable without expanding the product into an observability platform. Implement real structured JSON logging, remove unconditional library printing, classify persistent listener and I/O failures, and define stable event semantics for production static-serving profiles.

This plan begins Release E. It depends on Plans 075–086 so logging reflects corrected runtime, configuration, body, filesystem, and HTTP behavior.

## Scope

Eggserve needs enough observability for operators and release qualification to determine:

- which security profile is active;
- why requests or connections were rejected;
- whether resources and shutdown behave correctly;
- whether TLS and filesystem errors are occurring;
- whether the process is entering a persistent failure loop.

It does not need an admin server, metrics endpoint, tracing backend, or vendor-specific telemetry integration.

## Preconditions

- Corrective Releases A–D are closed.
- Runtime lifecycle and connection metadata are truthful.
- Request/body/filesystem errors have stable internal categories.
- Configuration has one authoritative owner across Rust, CLI, and Python.

## Non-goals

Do not add:

- a Prometheus HTTP endpoint;
- OpenTelemetry exporter dependencies;
- remote log shipping;
- a control/admin socket;
- per-user analytics;
- distributed tracing;
- reverse-proxy trust or client-IP parsing;
- application-level logging APIs;
- arbitrary user-defined log schema plugins.

## Track A — Define the operational event model

Create an internal, versioned event taxonomy. At minimum include:

### Process and configuration

- process starting;
- effective version and source revision where available;
- selected support profile;
- bind address;
- root initialized;
- TLS enabled/disabled;
- unsafe/compatibility flags enabled;
- startup validation failure;
- listener ready;
- shutdown requested;
- draining started;
- forced shutdown started;
- shutdown complete with result.

### Connection lifecycle

- connection accepted;
- connection rejected due to admission limit;
- TLS handshake success/failure/timeout;
- header timeout;
- parser/framing rejection;
- keep-alive/connection closure reason;
- response write timeout or I/O failure;
- client disconnect;
- connection task panic/internal failure.

### Request/static service

- request completed;
- method/path category;
- status;
- bytes sent;
- duration;
- range/full/generated body kind;
- filesystem not found/denied/error;
- dotfile/symlink/reparse/root-escape denial;
- body policy rejection;
- service timeout/error;
- directory listing limit/error.

### Operational faults

- listener transient error;
- listener persistent/fatal error;
- handle/fd exhaustion;
- blocking worker saturation;
- log sink error;
- release/evidence inconsistency where runtime tooling uses the logger.

Every event must have:

- stable event name;
- severity;
- timestamp;
- process/session identifier where useful;
- optional connection/request correlation identifier;
- structured fields with documented types;
- sanitized human-readable message for text mode.

Do not promise schema stability beyond the declared version. Changes to field meaning require a schema-version update.

## Track B — Implement actual JSON Lines output

The CLI currently advertises `--log-format json`. Implement newline-delimited JSON where every emitted record is one complete JSON object.

Requirements:

- valid UTF-8 JSON;
- exactly one object per line;
- no text banners mixed into JSON mode;
- deterministic field names;
- timestamps in an explicit format;
- numeric values remain numbers;
- booleans remain booleans;
- absent optional fields are omitted or null according to one documented policy;
- control characters escaped by the JSON serializer;
- no manual string concatenation for JSON;
- logging failures do not recursively log indefinitely;
- stdout/stderr destination contract is documented.

Recommended destination policy:

- machine event stream on stderr by default so stdout remains available for CLI conventions;
- `--quiet` suppresses non-error startup text but does not suppress required operational errors unless `log-format none` is explicit;
- `none` truly disables request/startup logs while unavoidable fatal startup diagnostics still return through process error handling.

Add golden tests parsing every JSON line with a standard parser.

## Track C — Preserve safe text logging

Text mode should remain concise and human-readable.

Requirements:

- derive from the same event object as JSON mode;
- no separate semantic path that can drift;
- sanitize CR, LF, tabs, escape sequences, and other terminal-control characters;
- truncate or bound remotely controlled fields;
- never print raw request headers by default;
- never print authorization/cookie-like values;
- never print absolute root or resolved local paths in per-request logs;
- clearly identify unsafe startup flags.

Add injection tests for request targets and filenames containing control characters, bidi controls, and long inputs.

## Track D — Remove unconditional library output

Audit Rust core and Python native code for `println!`, `eprintln!`, `print`, direct stderr writes, and panic messages in normal operation.

Rules:

- library crates do not write to stdout/stderr during ordinary use;
- library APIs return typed errors or emit through an explicitly supplied observer/logger hook;
- CLI owns process-level presentation;
- Python package follows Python logging or callback semantics where exposed, without unsolicited output;
- panic hooks are not used as operational logging;
- tests may capture explicit test diagnostics but production paths remain silent unless configured.

Add source-level checks for prohibited printing macros in production modules, with narrowly documented exceptions.

## Track E — Listener error classification and backoff

The accept loop must not ignore persistent errors in a tight loop.

Classify listener errors into:

- transient/retryable;
- resource exhaustion;
- shutdown-related;
- fatal/non-recoverable;
- unknown with bounded retry.

Required behavior:

- transient errors use bounded exponential or capped backoff;
- backoff is interruptible by shutdown;
- resource exhaustion emits a rate-limited event and retries according to policy;
- fatal errors terminate the accept loop and move lifecycle state to a failed/stopped result;
- repeated identical events are rate-limited or summarized;
- no persistent error can cause unbounded CPU spin or log amplification.

Tests should inject/mock:

- interrupted accept;
- temporary unavailable condition;
- descriptor/handle exhaustion;
- listener closed unexpectedly;
- persistent synthetic error;
- shutdown during backoff.

Measure CPU/event count for a persistent error interval.

## Track F — Response and streaming error visibility

Ensure errors currently swallowed or ignored are categorized:

- file read failure after headers;
- seek/range failure;
- client disconnect;
- write timeout;
- TLS write/close failure;
- generated body failure;
- callback/service error;
- canonical response normalization failure.

Rules:

- expected client disconnects should not be logged as server errors by default;
- internal invariant failures should be error severity;
- errors after response commitment must not attempt an invalid second response;
- status/bytes-sent fields reflect what actually reached the transport where measurable;
- connection close reason is explicit.

## Track G — Correlation and privacy

Use bounded correlation identifiers generated internally.

Requirements:

- connection ID unique enough for one process lifetime;
- request sequence number per connection or request ID;
- no dependence on untrusted incoming request IDs;
- IDs contain no client data;
- no random generation that can block startup;
- wraparound behavior defined and safe.

Privacy/security rules:

- remote address may be logged according to explicit configuration and profile; do not infer client identity from forwarded headers;
- local address and TLS metadata may be included;
- request path logging uses a sanitized/truncated representation or category;
- query strings should be omitted by default or separately controlled because they may contain secrets;
- filesystem absolute paths are startup-only diagnostics at most and should be suppressible.

## Track H — Python observer/logging parity

For the in-process Python server:

- provide either a documented Python logging integration or a bounded event callback;
- do not execute arbitrary Python callbacks on critical I/O threads without bounded concurrency;
- callback failures must not crash the server;
- observer backpressure must be bounded and have a drop/coalescing policy;
- event schema should match Rust/CLI semantics;
- no event should expose raw PyO3 or Hyper types.

A minimal approach may expose structured dictionaries through an optional callback while keeping default behavior silent. Do not turn this into a middleware/event bus.

## Track I — Log sink failure behavior

Test closed pipes, disk/full redirection environments, broken stderr, and Python callback failures.

Required behavior:

- request processing does not panic because logging failed;
- failures are counted/coalesced internally where possible;
- no recursive attempt to log the logging failure indefinitely;
- process exit behavior for fatal startup diagnostics is deterministic;
- high-rate logging remains bounded by synchronous write behavior or a bounded queue.

If an asynchronous logger is used, define:

- queue capacity;
- drop policy;
- shutdown flush deadline;
- ownership/task lifecycle;
- behavior after sink failure.

Prefer a simple synchronous or bounded design over a complex telemetry subsystem.

## Track J — Operational counters without a network endpoint

Expose internal counters through existing library handles, release-test hooks, or structured summary events:

- accepted connections;
- admission rejects;
- active connections;
- active file streams/listings;
- parser/framing rejects;
- timeout categories;
- bytes sent;
- graceful/forced shutdown counts;
- listener errors;
- dropped log events if applicable.

These counters support soak and release tests in Plan 089. They need not be stable public API unless explicitly classified.

## Required tests

At minimum:

- every JSON line parses;
- JSON mode contains no plain-text banner;
- text/JSON events derive from equivalent semantic events;
- control-character and terminal injection corpus;
- path/query/header privacy checks;
- no absolute path leakage in request events;
- no library stdout/stderr during normal embedding;
- persistent listener error uses backoff and does not spin;
- shutdown interrupts backoff;
- client disconnect severity/category;
- response streaming error after headers;
- log sink failure;
- Python observer failure/backpressure;
- correlation IDs unique and bounded;
- installed binary/wheel logging parity.

## Property and fuzz tests

Fuzz the event serialization boundary with arbitrary sanitized field values. Assert:

- valid JSON;
- one record per line;
- bounded record size;
- no raw CR/LF injection into text record structure;
- no panic;
- deterministic truncation.

## Release-gate changes

Add gates such as:

- `ops.json-log-validity`;
- `ops.text-log-sanitization`;
- `ops.library-silence`;
- `ops.listener-backoff`;
- `ops.streaming-error-events`;
- `ops.log-sink-failure`;
- `ops.python-observer-parity`;
- `ops.installed-artifact-logging`.

Changes to event schema, runtime errors, CLI logging, Python observer code, or response streaming invalidate these gates.

## Documentation changes

Update:

- CLI reference;
- deployment guide;
- operations/logging guide;
- security policy;
- threat model;
- Python API;
- Rust API stability inventory;
- release contract;
- support-profile requirements;
- migration notes for corrected JSON behavior.

Publish example JSON events and clearly mark the schema version.

## Acceptance criteria

- `--log-format json` emits valid JSON Lines only.
- Text and JSON modes represent the same event semantics.
- Library embedding is silent unless an observer is configured.
- Persistent listener errors cannot spin or flood logs.
- Streaming and shutdown failures are categorized.
- Remote input cannot inject log records or terminal controls.
- Sensitive headers, queries, and absolute paths are not exposed by default.
- Python observer behavior is bounded and failure-safe.
- Counters/events support Plan 089 resource qualification.
- Exact-SHA installed-artifact tests pass.

## Stop conditions

Stop rather than adding complexity if:

- structured output requires an unbounded logging subsystem;
- the schema cannot be generated from one shared event model;
- observer callbacks can block critical runtime progress without a safe bound;
- log sink failure can still panic or deadlock;
- privacy guarantees cannot be tested deterministically.

## Handoff

After this plan closes, Plan 088 may optimize streaming and allocation behavior using the stable counters and event model. Plan 089 uses these events as part of soak and production release evidence.