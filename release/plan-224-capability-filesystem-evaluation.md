# Plan 224 — Capability-filesystem crate evaluation (NO-GO)

## Decision

**NO-GO.** Do not create `eggserve-capfs` / `eggcapfs` (or any equivalent
capability-filesystem crate). `eggserve-static` remains the single
path/filesystem confinement authority established by Plan 219.

This is an evidence-based evaluation gate, not a deferral for lack of review.
No new crate is created by this plan.

## Preconditions met

Plan 219 is complete: one authority exists for pinned root, Unix
fd-relative traversal, Windows handle-relative traversal, child open/listing,
reparse/symlink denial, and resolved file/directory capabilities
(`eggserve-static/src/fs/`, `src/secure_root.rs`, `src/path/`, `src/mime.rs`,
`src/planner.rs`; `eggserve-core` keeps facades only with `src/fs`,
`src/path`, `src/mime.rs` deleted and topology-gated).

## Evidence

### 1. The capability API is not independent of HTTP/static policy

The filesystem layer is deeply coupled to eggserve-specific types:

- `fs/mod.rs` imports `crate::path::{ConfinedPath, PathRejection}` and takes
  `&ConfinedPath` in `RootGuard::resolve` — the resolver consumes the HTTP
  request-target parse product directly.
- `fs/unix.rs` and `fs/mod.rs` take `&StaticPolicy` (serving-level
  `SymlinkPolicy` / `DotfilePolicy`) and return `PathRejection::DotfileDenied`
  / `SymlinkDenied` — serving policy, not neutral filesystem vocabulary.
- `fs::ResolvedFile::into_body` maps `BodyPlan` to `BodySource` and calls
  `crate::mime::mime_for_path` — MIME selection and HTTP body planning live
  inside the resolver's output type.
- Child validation (`validate_child_component_with_policy`,
  `may_be_short_name_alias`, `fallback_reverify`) duplicates parse-level
  component checks (control characters, percent-encoded dot-segments,
  backslash, Windows reserved names / ADS / drive prefixes, 8.3-alias
  re-check) as intentional defense in depth.

A neutral crate would either take `ConfinedPath`/`StaticPolicy` (leaking
eggserve policy everywhere) or take raw components plus generic policy hooks
(splitting the duplicated validation across two crates and risking
divergence). Both outcomes trip the plan's do-not-extract conditions.

### 2. Unsafe/FFI isolation does not materially improve

- Production unsafe is already confined to one module:
  `eggserve-static/src/fs/windows.rs` (`#![allow(unsafe_code)]`, every block
  with a local `SAFETY` comment). Unix traversal uses safe `rustix` wrappers.
- The only other unsafe in the crate is test-only Unix FIFO fixtures in
  `fs/mod.rs` (`#![cfg_attr(test, allow(unsafe_code))]`).
- The workspace `unsafe_code = "deny"` policy plus the two reviewed
  production exceptions (static Windows confinement, core systemd listener)
  already hold. Extraction would relocate the same ~40 unsafe blocks without
  reducing them, while `eggserve-static` would still transitively depend on
  `rustix` / `windows-sys`.

### 3. `eggserve-static` does not become materially simpler

`fs/` is roughly half of `eggserve-static` by line count, but the remainder
(`SecureRoot`/`ResolvedFile`/`ResolvedDirectory` wrappers, planner, path,
MIME, `StaticService`) exists to consume exactly those `pub(crate)` types.
Extraction would promote `PinnedRoot`, `RootGuard`, `ResolvedFile`,
`ResolvedDirectory`, and `ResolvedResource` to a public cross-crate API,
add versioning/publishing/topology-gate surface, and keep thin wrappers in
static — relocating complexity, not removing it.

### 4. No second consumer exists; no independent review benefit demonstrated

Current `SecureRoot` consumers are all static paths: leaf `StaticService`,
core compatibility facades, and the Python bridge (including the
`python-bindings-internal` capability bridge). The cross-repo program
(Plans 222/223) reuses TLS identity and the outbound CONNECT wire primitive
in eggress/eggfetch — neither consumes filesystem confinement. No concrete
neutral consumer, fuzzer-only consumer, or review request justifies a new
published boundary.

### 5. No-reopen invariant is already satisfied in place

The resolution-path audit (`architecture/filesystem-confinement.md`) proves
no serving path reopens a reconstructed path: `safe_relative_components` is
MIME-only, `construct_path` builds logical verification paths, and
`resolve_child` re-resolves from the parent descriptor/handle. A new crate
would have to re-prove the same invariant over a new API with no new
capability gained — `SecureRoot` already is the capability API.

## Criteria scorecard

Extract-if (all required; none met except the last):

- [ ] Small capability API independent of HTTP — **no** (ConfinedPath,
      StaticPolicy, BodySource, MIME coupled).
- [ ] Unsafe/FFI isolation materially improves auditability — **no** (one
      module already; same blocks relocated).
- [ ] `eggserve-static` becomes materially simpler — **no** (relocation +
      new public surface).
- [ ] Concrete second consumer or substantial review benefit — **no**.
- [x] No path reconstruction/reopen API needed — yes, but already satisfied
      in place; extraction adds risk without capability gain.

Do-not-extract (any sufficient; all four hold):

- [x] API leaks eggserve path/static policy.
- [x] New crate mostly re-exports internal types.
- [x] Requires generic policy hooks that weaken auditability of the
      defense-in-depth duplication.
- [x] No consumer beyond `eggserve-static`; audit surface unchanged.

## Consequences

- No new crate, dependency, feature flag, or versioned API.
- `eggserve-static` stays the sole implementation owner of path parsing,
  secure-root resolution, filesystem confinement, MIME, and static response
  planning. `eggserve-core` stays facades-only (topology-gated).
- `docs/non-goals.md` ("No crate split without measured benefit") remains the
  standing gate for any future split proposal.
- The topology check gains a narrow NO-GO guard: fail if a
  capability-filesystem crate (`eggserve-capfs`, `eggcapfs`, `capfs`)
  appears in the workspace graph, so a future split requires an explicit
  plan and gate update rather than silent creation.

## Revisit conditions

Reopen only with fresh evidence that all of the following hold:

1. A concrete second in-workspace consumer with neutral (non-HTTP,
   non-static-policy) requirements exists — not a hypothetical reuse.
2. A proposed neutral component API preserves the current defense-in-depth
   duplication without splitting validation across crates (or justifies the
   new split with parity evidence stronger than today's in-crate tests).
3. Measurements satisfy `docs/non-goals.md`: default artifacts or compile
   graph materially benefit, compatibility migrates simply pre-1.0, and
   workspace/release complexity does not grow disproportionately.
4. The proposed API provably cannot reopen resources from reconstructed
   paths (resolution-path audit equivalent to today's), and unsafe/FFI
   ownership moves entirely (not duplicated) into the new crate.

## Verification

- `python3 scripts/check-crate-topology.py` (includes the Plan 224 NO-GO
  guard: no capfs crate in the resolved graph; static authority markers
  unchanged via the Plan 219 rules).
- `cargo test -p eggserve-static` plus the existing authority conformance
  fixture (`crates/eggserve-core/tests/static_authority_conformance.rs`);
  no new crate means no new test target is required.
