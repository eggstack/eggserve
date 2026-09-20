# Plan 234 handoff decisions

| Plan | Baseline input | Handoff |
| --- | --- | --- |
| 235 | RequestTarget owns path/query copies; complete bodies transition; fixed bodies eagerly own trailer slots; `get_unique` allocates `Vec` | GO; each replacement is private and mechanically reviewable |
| 236 | Hardened Unix non-root lookup clones the pinned root; normalized paths pass through decoder/normalizer temporaries; range parsing collects one item | GO; preserve every stat/open/type check |
| 237 | Both H1 adapters retain an outer `Arc<dyn Fn>`; ordinary deadline checks lock the tunnel JoinSet | GO for dispatch and zero-tunnel flag; DEFER metadata sharing unless a clean boundary is demonstrated |
| 238 | Runtime interim sender allocation is separate from lifecycle state | Conditional; likely NO-GO if lazy sharing enlarges common state or couples locks |
| 239 | Python request eagerly materializes byte/text compatibility views; one producer thread per active synchronous stream | GO for lazy views; DEFER stream-worker redesign until bounded isolation can be proved |

The baseline did not claim a numeric allocation count because the host lacks a
usable allocation profiler. Logical allocation claims are limited to source-
mechanical observations and are rechecked by the candidate tests.

Plan 241 reconciliation: this handoff's original static/profiling scope is
unchanged. The later custom H1, path-shape, TLS, installed-wheel Python,
slow-stream, and focused syscall measurements were completed by
`benchmarks/241-fixed-cost-evidence-corrective/`, not retroactively attributed
to this baseline.
