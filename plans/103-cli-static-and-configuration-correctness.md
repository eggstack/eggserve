# Plan 103 — CLI, Static-Serving, and Configuration Correctness

## Status

First execution plan under Plan 102. **COMPLETE.**

This plan closes independently actionable correctness defects in the CLI, static-serving path, limits model, directory-listing configuration, index fallback, and Python package metadata. It must not redesign the generic service runtime; that work belongs to Plan 104.

Baseline roadmap commit:

```text
26612813b25783188eb1529e29b6dca3e7755a22  Plan 102
```

## Goal

Make the existing local static-server product truthful and fail-closed before changing the generic runtime architecture.

Required outcomes:

1. `--quiet` suppresses routine informational output while retaining actionable errors.
2. `--log-format none` disables structured operational output rather than selecting text output.
3. no accepted concurrency configuration can panic Tokio semaphore construction.
4. every retained directory-listing limit is enforced at its actual boundary.
5. static index fallback consistently checks `index.html`, then `index.htm`.
6. rejected static request bodies do not consume a hard-coded five-second drain budget.
7. Python package license metadata is internally consistent.
8. focused tests prove each correction without expanding normal CI.

## Governing constraints

- Do not alter root-confinement algorithms.
- Do not add a logging framework or tracing dependency.
- Do not add dynamic logger reconfiguration.
- Do not add new CLI flags.
- Do not add a generic configurable index-page list to the CLI.
- Do not add directory listing by default.
- Do not add a general HTML templating layer.
- Do not create a separate listing worker pool.
- Do not add new CI jobs or workflows.
- Do not broaden Python version or platform policy in this phase.
- Do not change generic custom-service body semantics here; Plan 104 owns that boundary.

## Current defects to correct

### Logger truthfulness

The binary currently derives a `_quiet` value but does not use it. Logger initialization maps every non-JSON mode, including `none`, to text output and emits startup/lifecycle events unconditionally.

This produces two user-visible contract failures:

- `--quiet` does not suppress the startup banner and routine events;
- `--log-format none` does not disable logging.

### Semaphore upper bounds

`Limits::validate()` checks that connection and file-stream counts are nonzero but accepts values above `tokio::sync::Semaphore::MAX_PERMITS`. Construction can therefore panic after public validation succeeds.

Public validation must make panic-by-configuration impossible.

### Listing limits

The public limits model currently exposes:

- maximum entries;
- maximum listing response bytes;
- maximum encoded filename bytes;
- listing enumeration timeout.

Only the entry limit is visibly enforced in the reviewed static-serving path. Security-looking fields that are not enforced must not remain public.

This plan chooses a deliberately small contract:

- retain `max_listing_entries` as the configurable enumeration bound;
- retain and enforce `max_listing_response_bytes` as the final encoded-body bound;
- remove `max_listing_filename_bytes` from the public limits model unless existing resolver code already enforces it at enumeration time;
- remove `listing_enumeration_timeout` unless enumeration is actually moved to a cancellable blocking boundary without additional machinery.

Do not add a worker architecture solely to preserve an unused timeout field.

### Index fallback

The supported static behavior should be fixed and predictable:

```text
1. index.html
2. index.htm
3. directory listing if explicitly enabled
4. otherwise forbidden
```

Both the direct Rust static service and the compatibility/native static responder must follow this order.

### Rejected-body drain

The static server rejects request bodies. A hard-coded pre-service drain can spend resources consuming data the server has already refused.

The static path should return the appropriate final error, mark/produce connection-close semantics, and stop processing the rejected content. Do not add a configurable drain mode.

## Track A — Make logging modes truthful

### Required implementation

Introduce the smallest logger behavior that supports three effective modes:

```text
text  -> text sink
json  -> JSON sink
none  -> null/no-op sink
```

Acceptable implementations:

- add a small `NullLogSink` implementing the existing sink trait;
- or configure the existing logger with an explicit disabled sink.

Do not scatter `if log_format != None` checks around every event site.

### `--quiet` semantics

`--quiet` is distinct from `--log-format none`.

Required behavior:

- suppress startup banner and routine informational lifecycle messages;
- preserve warnings and errors;
- preserve nonzero exit status and explicit configuration/startup errors;
- do not suppress panic output or Rust runtime failures.

Implement this with a severity threshold or a small filtering sink around the existing logger. Do not create per-module quiet checks.

### Interaction rules

```text
--log-format text              text info/warn/error output
--log-format json              JSON info/warn/error output
--quiet                        selected format, warn/error only
--log-format none              no structured operational output
--quiet --log-format none      same as none
```

Direct argument-validation errors printed before logger initialization may remain on stderr. `none` is not a request to make invalid invocation silent.

### Required tests

Add focused binary/integration tests for:

- default mode emits a listener/startup event;
- JSON mode emits valid JSON Lines at least for startup;
- quiet mode omits the normal listener/startup message;
- quiet mode still emits a forced startup error;
- none mode emits no routine output during start/stop;
- none mode still reports an invalid CLI invocation before startup;
- `--quiet --log-format json` does not emit informational JSON records.

Use a temporary root and port `0` or the existing controlled-start seam. Avoid wall-clock sleeps beyond existing readiness mechanisms.

### Acceptance criteria for Track A

- `_quiet` is removed or becomes an effective input.
- `none` never initializes a text sink.
- event call sites remain structurally unchanged where possible.
- logger behavior is documented in one authoritative CLI section.

## Track B — Validate semaphore construction bounds

### Required implementation

Update `Limits::validate()` to reject:

```text
max_connections > tokio::sync::Semaphore::MAX_PERMITS
max_file_streams > tokio::sync::Semaphore::MAX_PERMITS
```

Use the Tokio constant directly. Do not duplicate its numeric value.

Update `RuntimeConfigBuilder::build()` to apply the same checks, preferably through one shared helper rather than independent string-only validation.

Update `ServeState::new()` and every other public state-construction boundary to validate before constructing semaphores. The state constructor must not assume all callers used a particular builder.

### Error contract

Return a controlled configuration error naming:

- the field;
- the rejected value;
- the maximum supported value.

Do not panic, saturate, silently clamp, or wrap the count.

### Required tests

Add tests covering:

- exactly `Semaphore::MAX_PERMITS` succeeds where memory-independent construction permits;
- `MAX_PERMITS + 1` is rejected without constructing state;
- `usize::MAX` is rejected;
- zero remains rejected;
- both connection and file-stream fields are covered;
- direct `ServeState::new()` with invalid limits returns an error rather than panicking;
- runtime builder and `ServeConfig` conversion report consistent field names.

Where `MAX_PERMITS + 1` could overflow on an unusual target, use checked arithmetic and conditionally structure the test.

### Acceptance criteria for Track B

- no validated configuration can panic `Semaphore::new()`.
- all public constructors reject the same invalid values.
- no arbitrary lower operational maximum is introduced without evidence.

## Track C — Reconcile directory-listing bounds

### Inventory first

Before editing fields, search all production, Python-binding, CLI, documentation, test, benchmark, and fuzz references to:

```text
max_listing_entries
max_listing_response_bytes
max_listing_filename_bytes
listing_enumeration_timeout
```

Record the actual enforcement point or absence for each field in the implementation commit message or Plan 106 closure record.

### Required retained contract

#### `max_listing_entries`

Retain and enforce during directory enumeration before constructing the response body.

Behavior at the boundary must be deterministic. Choose one existing-compatible result and document it:

- either reject the listing with a controlled 413/500-style server response;
- or truncate only if the current documented contract already promises truncation.

Prefer rejection over silent truncation because partial directory views can mislead users.

#### `max_listing_response_bytes`

Retain and enforce while constructing the encoded HTML response.

Required construction behavior:

- use checked length growth;
- include fixed HTML prefix/suffix in the bound;
- include escaped visible text and percent-encoded link bytes;
- stop before allocating or appending beyond the configured maximum;
- return a controlled error response without a partial listing body.

Do not build the full unbounded string and check its size afterward.

### Fields to remove unless already effective

#### `max_listing_filename_bytes`

Remove if it is not already enforced by the resolver/enumerator. Filesystem filename limits and encoded HTML expansion are separate concerns, and the final response-size bound is the relevant server resource control.

Do not add a second arbitrary filename policy merely to preserve the field.

#### `listing_enumeration_timeout`

Remove if enumeration is synchronous and not cancellable at the current boundary.

Do not introduce `spawn_blocking`, cancellation plumbing, a worker pool, or platform-specific interruption solely to make this field nominally effective. The entry bound is the proportional safeguard for this product.

### API and documentation updates

When removing fields:

- update `Limits` construction and defaults;
- update runtime/static conversion code;
- update Python binding constructors only if they expose the fields;
- update examples, architecture tables, API docs, tests, and counts;
- do not add compatibility aliases.

This is an alpha API correction.

### Required tests

Add deterministic tests for:

- exactly the entry limit succeeds;
- one entry beyond the limit returns the documented response;
- HTML escaping expansion is counted against the response-byte limit;
- percent-encoding expansion is counted;
- fixed prefix/suffix are counted;
- a response that would exceed the limit does not expose a partial `<ul>`;
- HEAD listing retains equivalent GET metadata and sends no body;
- removed fields no longer appear in public compile samples or Python signatures.

### Acceptance criteria for Track C

- every retained listing limit has a direct production enforcement test.
- every removed field disappears from active documentation and public constructors.
- no additional worker/runtime subsystem is created.

## Track D — Unify static index fallback

### Required implementation

Create one small static index-name constant or helper shared by the Rust static paths where practical:

```text
index.html
index.htm
```

For each candidate:

- resolve through the existing confined root/child-resolution mechanism;
- apply dotfile and symlink policy;
- use the opened handle returned by the resolver;
- do not join a filesystem pathname and reopen it;
- preserve conditional, range, HEAD, MIME, and file-stream behavior.

If Python compatibility supports an explicitly bounded `index_pages` override, keep that compatibility behavior in its native responder. The default must match the fixed Rust order.

Do not expose a new Rust CLI option for arbitrary index names.

### Required tests

Cover:

- `index.html` wins when both files exist;
- `index.htm` is served when `index.html` is absent;
- a denied/symlinked first candidate does not bypass policy;
- conditional GET works for `index.htm`;
- range works for `index.htm`;
- HEAD metadata matches GET for `index.htm`;
- MIME is selected from the actual index file;
- directory listing is reached only after both candidates are absent;
- default Python compatibility ordering matches direct Rust serving.

### Acceptance criteria for Track D

- direct and compatibility default behavior match.
- path confinement remains handle-based.
- no generalized index-routing subsystem is introduced.

## Track E — Remove fixed drain behavior for rejected static bodies

### Boundary decision

Static serving rejects bodies for GET and HEAD as a product policy. When such a request carries content:

- reject before invoking static resolution;
- generate the existing controlled error status;
- ensure the connection is not reused with unread content;
- do not read and discard content for a fixed five seconds.

The generic service body-policy implementation is corrected in Plan 104. This track should make only the minimum static-facing change that is safe before that architecture lands.

### Required implementation

Preferred behavior:

- return 400 or 413 according to existing framing/size semantics;
- add `Connection: close` where the transport requires explicit closure;
- drop the incoming body/connection after the response;
- retain early rejection of `Expect: 100-continue` without sending an interim invitation.

Do not add a drain timeout flag or a `Drain` policy variant.

### Required tests

Add wire tests proving:

- a body-bearing GET receives the documented error;
- the handler/static resolver is not invoked;
- the connection is closed and cannot carry a smuggled follow-up request;
- `Expect: 100-continue` does not receive `100 Continue` before rejection;
- rejection completes without waiting for the old fixed drain interval;
- valid bodyless GET/HEAD behavior is unchanged.

The timing test should use a broad deterministic upper bound only to prove the removed five-second behavior; avoid millisecond-precision assertions.

### Acceptance criteria for Track E

- no hard-coded rejected-body drain remains in the static path.
- unread bytes cannot be interpreted as a subsequent request.
- no new body-policy mode is introduced.

## Track F — Correct Python package metadata

### Required implementation

Reconcile `crates/eggserve-python/pyproject.toml` with the repository's actual license.

If the project is MIT-only:

- retain the MIT license declaration/classifier;
- remove the Apache Software License classifier.

If repository root metadata proves dual licensing, update the declaration consistently instead. Do not infer dual licensing from a stale classifier.

Do not broaden Python version support in this plan.

### Required validation

- build the wheel;
- inspect wheel metadata;
- confirm only the correct license classifier is present;
- confirm no tests or documentation claim a different license.

## Track G — Focused documentation and verification

### Required documentation

Update only the active files touched by this plan, including as applicable:

- `README.md` CLI behavior;
- `docs/cli.md`;
- `docs/security-policy.md`;
- `docs/python-http-server-compatibility.md`;
- `docs/python-api.md`;
- `architecture/configuration.md`;
- `architecture/response-planning.md` or static-serving equivalent;
- `architecture/testing-and-conformance.md` when counts change;
- module-level docs for changed `Limits` fields.

Do not rewrite the architecture overview again merely to update counts. Make the smallest accurate edits.

### Required local verification

At minimum run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Plan 106 may later simplify the routine commands. Plan 103 must validate against the repository as it exists when implemented.

### Required focused checks

- CLI capture tests for logging modes;
- limit validation tests;
- listing bound tests;
- direct static and Python index tests;
- rejected-body wire tests;
- wheel metadata inspection.

Do not run the entire deep suite solely because this plan changes bounded static behavior. Run the existing filesystem race tests only if root-resolution code is modified.

## Completion criteria

Plan 103 is complete when:

- logging flags are truthful;
- semaphore counts are fully bounded;
- listing configuration is honest and enforced;
- index fallback is consistent;
- rejected static bodies no longer trigger fixed draining;
- Python license metadata is correct;
- focused tests pass;
- active documentation matches the implementation;
- no generic runtime redesign or new infrastructure has been introduced.

## Explicit rejection criteria

Reject the implementation if it:

- suppresses startup errors under `--log-format none`;
- silently clamps concurrency values;
- retains unused listing fields;
- adds a thread pool or asynchronous listing subsystem;
- reopens an index file by pathname after policy authorization;
- enables directory listing by default;
- makes index names user-configurable in the Rust CLI;
- keeps the five-second body drain under a different constant;
- adds a new CI workflow;
- changes custom-service body semantics before Plan 104.

## Handoff note

After Plan 103 lands, proceed directly to Plan 104. Do not declare the Plan 102 roadmap complete based on this phase alone.
