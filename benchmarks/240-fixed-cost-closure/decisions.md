# Plan 240 closure decisions

| Track | Decision | Basis |
| --- | --- | --- |
| Plan 235: request target | KEEP | `RequestTarget` keeps the validated raw bytes and exposes path/query slices; public parsing, empty-query, and byte-fidelity tests pass. |
| Plan 235: request body | KEEP | terminal lifecycle construction avoids a transition for already-complete in-memory bodies; lazy wire-trailer state preserves shared-slot identity and finalization semantics. |
| Plan 235: header lookup | KEEP | `get_unique` uses one bounded scan and only allocates the duplicate error name when it must report a duplicate. |
| Plan 236: Unix resolver | KEEP | first-component lookup borrows the pinned root descriptor; nested traversal remains owned and retains `statat`/`openat(O_NOFOLLOW)` checks. |
| Plan 236: path/range | KEEP | safe normalized paths avoid decoder/normalizer temporaries on the common form; range parsing uses iterator lookahead without changing accepted ranges. |
| Plan 237: H1 dispatch | KEEP | the internal service adapter is generic rather than an outer `Arc<dyn Fn>`; workspace tests and direct/compatibility paths pass. |
| Plan 237: zero-tunnel activity | KEEP | ordinary deadline/count checks skip the tunnel-set mutex; `JoinSet` remains the single tunnel owner and drainer. |
| Plan 237: metadata sharing | DEFER | no API-neutral representation demonstrated a meaningful win; connection metadata remains explicit and readable. |
| Plan 238: shared request state | NO-GO | merging interim sender and lifecycle state would couple unrelated locks or enlarge the common request object without a measured allocation win. The standalone interim state is retained. |
| Plan 239: Python request views | KEEP | target, text headers, ordered headers, and byte views are lazy canonical-derived compatibility views; excluded-crate compilation passes and opaque values retain byte access. |
| Plan 239: stream producer redesign | DEFER | the dedicated synchronous producer thread remains the bounded isolation/backpressure design; replacing it needs separate resource and cancellation evidence. |

The candidate native matrix had zero errors in all nine cases. It shows
ordinary same-machine variance rather than a universal throughput result, so
no timing claim is promoted to CI. The implementation is retained because
the deterministic correctness/resource suite passes and each change is a
local representation or common-path simplification with preserved security,
framing, lifecycle, timeout, and admission behavior.

Plan 241 closes the acceptance items that this record deliberately left
unmeasured. See its `decisions.md` for the explicit CONFIRMS/NEUTRAL/N/A
reconciliation; no retained production decision changes.
