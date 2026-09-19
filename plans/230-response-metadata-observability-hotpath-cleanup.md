# Plan 230 — Response finalization, request metadata, and observability hot-path cleanup

## Prerequisite

Plan 227 must establish whether metadata/event construction is visible enough
to justify hot-path changes. Tracks in this plan may be skipped individually if
the profile shows they are below noise and the change would add complexity.

## Purpose

Remove repeated small work from ordinary requests/responses while preserving
EggServe's canonical transport boundary, response privacy policy, and
structured observability semantics.

The planning review identified three concrete classes:

1. contextual canonical-to-Hyper conversion currently adds a default `Date`,
   after which the runtime finalizer removes/replaces it according to
   `ResponsePolicy::date_policy`;
2. request validation creates temporary collections/copies that are unnecessary
   for the common case;
3. `Event::new` constructs a timestamp/message/field vector before a no-op or
   severity-filtered sink can discard the event.

## Goals

- Make the runtime response-policy boundary the only runtime `Date` authority.
- Preserve standalone adapter behavior for callers that do not have a runtime
  policy.
- Avoid temporary request metadata allocations where existing iterators suffice.
- Let disabled/filtered structured events avoid timestamp/string/field
  construction.
- Preserve the public logging/event API source compatibility where practical.
- Keep canonical primitives independent of Hyper/http runtime types.

## Work

### 1. Collapse duplicate runtime Date generation

Refactor the outbound adapter boundary so these cases are explicit:

- standalone `to_hyper_response(...)`: retain the documented default origin
  `Date` behavior;
- runtime contextual conversion: do not synthesize a temporary `Date`;
- `finalize_runtime_response`: remains the sole runtime authority for
  `DatePolicy::SystemClock`, `Suppress`, and `Custom`.

Do not make Hyper automatic Date generation authoritative; it remains disabled.

Add wire/header tests proving:

- standard runtime responses contain exactly one Date;
- suppress policy contains none;
- custom policy uses the supplied provider exactly as specified;
- application-supplied Date remains subordinate;
- standalone adapter behavior remains compatible;
- future `Last-Modified` suppression still compares against the authoritative
  runtime Date.

### 2. Evaluate one-second Date caching separately

Only after duplicate generation is removed, profile the remaining
`SystemClock + fmt_http_date` cost.

A one-second cache may be implemented only if:

- it is process/runtime safe under concurrency;
- it never emits a Date from a later second;
- it does not alter `Custom` provider call semantics;
- it does not affect `Suppress`;
- tests can deterministically verify rollover behavior.

If the residual cost is negligible, do not add a cache.

### 3. Request HeaderBlock preallocation

In canonical request conversion:

- compute/use the already-known Hyper header count;
- initialize `HeaderBlock::with_capacity(req.headers().len())`;
- preserve aggregate-byte checks before service dispatch;
- preserve byte-exact header values and duplicate order.

### 4. Remove temporary Content-Length collection

Rewrite body-framing duplicate/conflict validation to inspect the
`get_all(Content-Length)` iterator without collecting a `Vec<&HeaderValue>`.

Preserve:

- TE+CL defense-in-depth behavior;
- duplicate agreeing CL rejection;
- duplicate conflicting CL rejection;
- sanitized errors.

### 5. Remove temporary Host-authority collection

Validate Host authority multiplicity/consistency with a streaming/first-value
approach rather than collecting every parsed authority into a `Vec`.

Requirements:

- zero Host remains valid/invalid exactly where current protocol rules say;
- one Host is preserved;
- duplicate equivalent Hosts retain current behavior;
- conflicting Hosts reject;
- URI authority consistency rules remain unchanged;
- no hostile value is logged unsanitized.

### 6. Lazy disabled-event path

Add a source-compatible severity capability to `LogSink`, preferably a
default method so downstream implementations compile unchanged, for example:

```rust
fn enabled(&self, severity: Severity) -> bool { true }
```

Expected implementations:

- `NopLogSink`: false;
- `FilteredLogSink`: compare against minimum severity and delegate;
- `CompositeLogSink`: true if any child can consume the event;
- `StderrLogSink`: true.

Expose an `OpsContext` lazy emission helper that accepts a closure and invokes
it only when the sink is enabled. Keep panic containment around actual sink
execution.

Convert only hot/high-frequency sites first, prioritizing:

- normal connection lifecycle debug events;
- response stream started/completed/cancelled debug events;
- accepted forwarded-metadata debug events if enabled by policy;
- other events shown by Plan 227 profiles.

Counters remain unconditional relaxed atomics. Warn/error paths may remain eager
unless profiling warrants conversion.

### 7. Avoid hidden behavior changes in event filtering

Tests must prove:

- disabled events do not evaluate the event-building closure;
- enabled events retain schema/timestamp/message/fields;
- composite sinks deliver to enabled children correctly;
- a panicking enabled sink is still contained and counted;
- disabled children do not force eager event construction;
- filtering does not change counters;
- logger/global compatibility behavior remains unchanged.

### 8. Minor metadata cleanups only when simple

After the above, profile before touching additional code. Acceptable small
changes include:

- pre-sizing response/header vectors where the exact count is known;
- avoiding repeated `String` creation for static internal field names through
  existing `&'static str`/constructor paths when no public representation
  change is required.

Do not add unchecked/unsafe header constructors merely to avoid validation.
Canonical transport validation is an invariant.

## Measurement requirements

A/B:

- 1 KiB static and custom bytes at concurrency 1/16/64;
- HEAD/304 response-heavy workload;
- no-op logging vs stderr/filtered logging microbenchmarks;
- one streaming workload to ensure lazy events do not alter stream lifecycle.

Record CPU, throughput, latency, allocation counts where available, and event
counts.

The Date and temporary-collection changes may be retained as straightforward
simplifications even if individually small, provided they do not add code
complexity. Lazy logging must show either measurable allocation/CPU reduction
for disabled debug logging or a clear removal of eager work with no regression.

## Non-goals

- No replacement of `HeaderBlock` with a map.
- No transport/http crate dependency in `eggserve-primitives`.
- No unchecked header validation shortcuts.
- No removal of counters or error/warn observability.
- No tracing dependency.
- No change to public log schema fields.
- No change to response privacy policy semantics.

## Acceptance criteria

- [ ] Runtime responses perform one Date-authority pass.
- [ ] Standalone adapter Date behavior remains compatible.
- [ ] Request HeaderBlock capacity uses known header count.
- [ ] Content-Length and Host validation avoid temporary vectors while
      preserving semantics.
- [ ] Disabled/filtered hot debug events can avoid event construction.
- [ ] Sink panic containment and dropped-event accounting remain correct.
- [ ] Canonical primitive dependency boundaries remain unchanged.
- [ ] Targeted A/B measurements and allocation evidence are recorded.
