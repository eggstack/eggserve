# Plan 224 — Evaluate a dedicated capability-filesystem crate after de-duplication

## Purpose

Decide whether the platform-specific filesystem-confinement machinery should become a dedicated reusable crate after Plan 219 has established exactly one implementation.

This is an evaluation gate, not an automatic extraction.

## Preconditions

Do not begin until Plan 219 is complete.

There must first be one authority for:
- pinned root,
- Unix fd-relative traversal,
- Windows handle-relative traversal,
- child open/listing,
- reparse/symlink denial,
- resolved file/directory capabilities.

## Question

Would extracting this machinery into a crate such as `eggserve-capfs` or a neutral `eggcapfs` materially improve:
- unsafe/FFI auditability,
- static-server dependency boundaries,
- independent testing/fuzzing,
- reuse by another eggstack project,

without creating a generic abstraction that weakens eggserve's confinement guarantees?

## Evaluation criteria

### Extract if

- the platform layer can expose a small capability API independent of HTTP,
- unsafe/Windows-FFI/rustix code becomes isolated behind one crate,
- `eggserve-static` becomes materially simpler,
- another concrete consumer exists or independent security review benefits are substantial,
- no path reconstruction/reopen API is needed.

### Do not extract if

- the API necessarily leaks eggserve-specific path/static policy everywhere,
- the new crate mostly re-exports internal types,
- it requires generic policy hooks that make confinement behavior harder to audit,
- no consumer beyond eggserve-static exists and audit surface does not improve.

## Candidate boundary

Potential neutral responsibilities:
- open/pin a trusted root,
- resolve validated path components relative to the pinned root,
- open child file/directory capabilities,
- list a directory relative to a directory capability,
- reject symlink/reparse traversal under hardened mode,
- expose metadata necessary for serving without reopening by path.

Keep in eggserve-static:
- HTTP request-target parsing,
- percent decoding policy,
- dotfile serving policy,
- MIME,
- conditional/range planning,
- directory HTML,
- HTTP error mapping.

This separation must be validated carefully because some component-validation behavior is intentionally duplicated at parse and filesystem layers as defense in depth.

## Unsafe policy objective

If extraction proceeds:
- capability crate owns all required platform FFI/unsafe exceptions,
- other production crates use `unsafe_code = "forbid"` where feasible,
- every unsafe block has a local safety invariant,
- Windows handle ownership and Unix fd ownership are explicit RAII types.

## Tests

Reuse all confinement suites directly against the capability layer, plus integration tests through `eggserve-static`.

Add property/race tests for:
- root replacement,
- intermediate symlink/reparse swaps,
- child replacement,
- directory enumeration/open races,
- invalid component rejection.

## Deliverable

Produce an ADR/review result with one of:
- GO: concrete API and migration plan,
- NO-GO: rationale and conditions that would justify revisiting later.

Do not create a new crate unless the gate concludes GO.

## Acceptance criteria

- Decision is evidence-based and documented.
- No duplicate resolver exists regardless of decision.
- If GO, the proposed API cannot reopen resources from reconstructed paths.
- If NO-GO, eggserve-static remains the single confinement authority.
