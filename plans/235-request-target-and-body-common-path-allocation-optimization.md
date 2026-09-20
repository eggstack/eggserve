# Plan 235 — Request-target and request-body common-path allocation optimization

## Prerequisite

Plan 234 must confirm the target allocations mechanically or through profiling.
This plan may still implement a mechanically proven allocation removal when the
end-to-end timing effect is below benchmark noise, but must record that fact.

## Purpose

Reduce fixed allocations in canonical request construction without changing the
public Rust API, accepted syntax, normalization semantics, body lifecycle, or
trailer behavior.

## Track A — single-buffer RequestTarget

Current `RequestTarget` owns `raw`, `path`, and optional `query` strings.
For origin-form requests the path/query are slices of the raw target, so this
duplicates immutable bytes.

Change only the private representation. A recommended shape is:

```text
raw: String
path_end: usize
query_start: Option<usize>
```

or an equivalent index representation.

Preserve exactly:

- `RequestTarget::parse(raw: impl Into<String>)`;
- `raw()`, `path()`, `query()`, `path_and_query()`;
- `raw_bytes()`, `path_bytes()`, `query_bytes()`;
- `Clone`, `Debug`, `PartialEq`, `Eq`, and `Display`;
- rejection order and error variants;
- `/path?` canonicalizing to `query() == None` while
  `path_and_query()` still returns the retained raw target;
- literal `#` behavior and all authority/absolute/asterisk rules.

Do not introduce unsafe slicing. Indices must be derived from byte positions
that are valid UTF-8 boundaries; `?` is ASCII and therefore safe.

Add focused tests proving pointer/range behavior where practical without making
pointer identity part of the public contract.

## Track B — direct complete lifecycle construction

`RequestShared::new_complete()` currently creates active state and transitions
it to complete. Construct the terminal state directly so a newly created body
does not perform an atomic transition or notify nonexistent waiters.

Preserve cancellation state, notification behavior for later observers, and
all body lifecycle accessors.

## Track C — lazy wire-trailer state

Bodyless and fixed in-memory bodies currently allocate a wire-trailer
`Arc<Mutex<...>>` even though ordinary use has no transport trailer producer.

Refactor private storage so the slot is lazily materialized for body kinds that
do not need it, while network-backed incoming bodies still share the exact slot
used by the transport adapter.

A `OnceLock<WireTrailerSlot>`, equivalent private lazy holder, or a
representation with the same thread-safety guarantees is acceptable.

Requirements:

- `new_wire_slot()` public behavior remains unchanged;
- `RequestBody::wire_slot()` returns the same logical shared slot on repeated
  calls;
- if a slot is obtained and populated, trailer finalization observes it;
- network body adapters do not gain an extra allocation or race;
- empty/fixed bodies that never request a slot allocate none for wire trailers;
- no unsafe code.

## Track D — iterator-only unique header lookup

Rewrite `HeaderBlock::get_unique` so it does not construct a temporary
`Vec<&HeaderValue>` merely to distinguish absent/single/duplicate values.

The implementation must:

- perform one bounded linear pass;
- return the first value for exactly one occurrence;
- preserve the exact duplicate count in `DuplicateHeaderError`;
- preserve case-insensitive matching and order;
- leave `get_all` unchanged for callers that explicitly request allocation.

## Tests

Add or extend deterministic tests for:

- target with no query, query, empty query, Unicode, percent bytes, literal
  `#`, and malformed forms;
- cloning/equality/display of the new target representation;
- empty/fixed/incoming body lifecycle transitions;
- lazy trailer slot identity and finalization;
- duplicate header counts for 0/1/2/many values.

Run fuzz/regression targets that exercise request-target parsing if present.

## Measurement

Against Plan 234 on the same machine/profile:

- request-target microbench with short/long/query targets;
- `RequestBody::empty` and fixed-body construction;
- canonical H1 1 KiB custom response at c1/c16/c64;
- static 1 KiB at c1/c16/c64;
- allocation counts per request.

Keep a change when it removes a mechanically unnecessary allocation/sync step
with equal or simpler code and all semantic tests pass, even if end-to-end RPS
is statistically neutral. Revert if object size, synchronization, or complexity
regresses materially.

## Non-goals

- No public request-target API redesign.
- No borrowed-lifetime request type.
- No removal of canonical validation.
- No header indexing/hash map added to `HeaderBlock`.
- No global object pool or custom allocator.

## Acceptance criteria

- [ ] Public request-target/body/header methods and behavior are unchanged.
- [ ] Normal request targets retain one owned target buffer rather than
      duplicating path/query strings.
- [ ] Already-complete body state is constructed terminally.
- [ ] Bodyless/fixed bodies do not eagerly allocate unused wire-trailer state.
- [ ] `get_unique` no longer allocates a temporary vector.
- [ ] Allocation evidence is recorded against Plan 234.
- [ ] Routine + feature-gated canonical/runtime tests pass.
