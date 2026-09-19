# eggserve-static

`eggserve-static` is the static-serving specialization. Since Plan 214 it
has owned the extracted `SecureRoot`, descriptor/handle-relative traversal,
dotfile/symlink policy, MIME selection, response planner, and `StaticService`
implementation that composes with `eggserve-server`. Plan 219 collapsed the
remaining duplication: the crate is now the **sole implementation authority**
for static path parsing (`path`: `ConfinedPath`/`PathPolicy`/`PathRejection`
plus decode/component/platform helpers), the pinned root and Unix/Windows
traversal (`fs`, crate-internal), resolved file/directory capabilities,
MIME selection, conditional/range planning, and directory listing
construction. `eggserve-core` keeps compatibility facades only
(`primitives::{SecureRoot, ConfinedPath, ...}` re-export the static types;
`src/fs`, `src/path`, and `src/mime.rs` are deleted), so security fixes land
once. The `python-bindings-internal` feature carries the narrow capability
bridge (`ResolvedFile::from_parts`/`into_parts`/`into_std_file`), which moves
an already-opened handle without reconstructing provenance; raw fd/handle
internals are never exposed.

The generic runtime has no edge to this crate. Applications that only need a
custom service can depend on `eggserve-server` and
`eggserve-primitives`; static serving is an explicit addition. The direct crate
is the hardened static implementation for new Rust consumers. Plan 221 makes
the first-party frontends consume it directly: the binary's unit tests drive
leaf `StaticService`, and the Python bridge resolves/plans through the leaf
(including the capability bridge) — with the extended static orchestration
(extra headers/error policy, listing budgets, `ServeConfig` validation)
staying compatibility-owned as documented orchestration under the Plan 225
facade closure. Behavior is covered by the existing qualification
suites plus the authority conformance fixture
(`crates/eggserve-core/tests/static_authority_conformance.rs`). No
pathname-based fallback is exposed by the direct static service.

Plan 224 evaluated extracting the platform confinement machinery into a
neutral `eggserve-capfs`/`eggcapfs` crate and closed NO-GO: the resolver
consumes `ConfinedPath`/`StaticPolicy`, returns `BodySource` with MIME
planning, intentionally duplicates parse-level validation as defense in
depth, already isolates production unsafe to `fs/windows.rs`, and has no
second consumer. A new crate would leak eggserve policy, mostly re-export
internal types, and split the audited validation without reducing
complexity. `eggserve-static` therefore remains the single confinement
authority with no new dependency, feature flag, or versioned API (see
`release/plan-224-capability-filesystem-evaluation.md`).
