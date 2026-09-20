# Plan 239 — Python bridge allocation and stream-resource optimization

## Prerequisite

Plan 234 must quantify both eager Python request materialization and
slow-stream thread/RSS behavior. Treat callback construction and streamed
response execution as separate subtracks.

## Purpose

Reduce Python-facade overhead without changing the existing Python-visible
properties, callback semantics, byte fidelity, bounded backpressure, or the
single native EggServe runtime.

## Track A — lazy Python request compatibility views

`PythonCallbackService::build_py_request` currently materializes several
parallel views eagerly: text method/path/query, ordered text headers, a
lowercased header map, raw target/path/query bytes, byte header pairs,
string/tuple endpoint forms, effective/proxy metadata, TLS strings, and
one-shot capability holders.

Inventory every Python-visible attribute and all in-repo consumers. Where
Plan 234 proves values are often unused, replace eager public fields with
property getters backed by canonical/shared data or private lazy caches.

Requirements:

- Python attribute names and returned value types do not change;
- frozen-class behavior remains;
- text header view still omits opaque non-UTF-8 values rather than coercing;
- ordered duplicate-preserving header items remain correct;
- byte views remain byte-exact;
- `headers` keeps its existing first-value/lowercase behavior;
- address string and tuple properties remain truthful for non-socket
  transports;
- TLS/proxy/effective metadata semantics remain unchanged;
- tunnel/interim/lifecycle one-shot/share semantics remain unchanged.

Prefer computing a cheap property on access over caching it if repeated access
is uncommon. Use `OnceLock`/private caches only where repeated conversion is
meaningful.

Do not hold the GIL across Rust blocking/network work.

## Track B — avoid gratuitous Python response copies

Audit response conversion for duplicate extraction/validation of:

- native `PyResponse` headers;
- structural response headers;
- bytes/text bodies;
- trailers.

Preserve duplicate/order behavior where the API provides it and runtime framing
authority. Avoid changing Python container types solely for speed.

## Track C — streamed-response thread resource qualification

Current synchronous `Response.stream` intentionally runs each Python iterator
on a dedicated native thread, acquires the GIL only to pull/copy one item, and
blocks outside the GIL on the bounded channel.

Do not replace this architecture merely because thread count is high.
Specifically:

- do not move long-lived backpressured producers onto Tokio worker threads;
- do not use `spawn_blocking` for arbitrarily long streams without proving
  blocking-pool isolation;
- do not introduce an unbounded producer queue;
- do not serialize unrelated streams through one worker.

If Plan 234 shows unacceptable scaling, produce a bounded internal design and
qualify it before switching. Possible directions include a dedicated bounded
stream-worker pool with fairness/cooperative handoff or explicit producer
admission, but any design must prove that a blocking/misbehaving iterator
cannot head-of-line block unrelated streams indefinitely.

If no design preserves the current isolation/backpressure semantics with a
clear resource win, record the thread model as an intentional tradeoff and
defer redesign to a separate plan.

Do not add a new public Python configuration knob under this plan merely to
paper over an internal scaling problem.

## Tests

- minimal callback reading no optional request properties;
- every request property individually and in combination;
- opaque header values;
- duplicate headers;
- no-socket and TLS/proxy metadata;
- request body + trailers;
- response bytes/text/stream/trailers;
- HEAD/body-forbidden stream suppression;
- iterator exception/non-bytes error behavior;
- disconnect while channel is full;
- shutdown with active streams;
- many simultaneous slow streams;
- async lowlevel shim parity where it reuses the same conversion helpers.

## Measurement

Against Plan 234:

- callback requests/s and latency for trivial handlers;
- allocations/temporary bytes during request construction;
- RSS under callback concurrency;
- thread count/RSS for 10/100/N active slow streams;
- shutdown and disconnect latency;
- GIL hold profile where practical;
- errors/truncation/backpressure evidence.

## Non-goals

- No ASGI product expansion.
- No new Python async runtime.
- No second accept loop.
- No public property removal/rename/type change.
- No unbounded queues.
- No production dependency solely for pooling/scheduling.

## Acceptance criteria

- [ ] Eager Python request copies identified by Plan 234 are removed or
      explicitly justified.
- [ ] Existing Python-visible request/response behavior passes wheel tests.
- [ ] Slow-stream resource evidence is retained.
- [ ] A producer-thread redesign lands only with boundedness, isolation,
      cancellation, and shutdown proofs; otherwise it is explicitly deferred.
- [ ] The async shim continues reusing the single conversion authority.
