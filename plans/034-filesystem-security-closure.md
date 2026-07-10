# Phase 34 — Filesystem Security Closure

## Goal

Close or explicitly delimit filesystem security guarantees across supported platforms. Verify that every serving path preserves resolver-opened capabilities and rejects unsafe resource types and child paths.

## Workstream A — Resolution-path audit

Trace all paths from request target to response body:

- root opening;
- component traversal;
- directory resolution/listing;
- index-file lookup;
- child resolution;
- static responder planning;
- callback-returned file bodies;
- full and range streaming.

Document where handles are opened, transferred, consumed, and released. Eliminate any serving path that reconstructs and reopens a filesystem path after secure resolution.

## Workstream B — Component and policy validation

Test child APIs against:

- empty, `.`, and `..` components;
- slash/backslash ambiguity;
- NUL;
- encoded separators;
- nested dotfiles;
- Unicode edge cases;
- oversized components and full targets;
- platform-reserved names.

Ensure direct child APIs cannot accept multi-component paths or bypass the normal policy layer.

## Workstream C — Resource-type restrictions

Verify regular-file-only serving across all platforms and resolution modes. Cover:

- FIFO;
- Unix socket;
- block/character devices where safely testable;
- procfs-like or synthetic special files where relevant;
- directory/file type changes;
- Windows reparse points and alternate data streams.

Unsafe resource types should resolve to a non-serving result without blocking on open/read.

## Workstream D — Race and mutation behavior

Add Unix-focused tests for:

- symlink replacement during traversal;
- directory replacement;
- rename/unlink after resolution;
- permission changes after open;
- file truncation during full/range streaming;
- disappearing directory entries;
- index-file replacement;
- hard-link behavior where it affects the stated threat model.

Tests should prove the server either continues using the opened capability safely or terminates the response without path re-resolution.

## Workstream E — Capability boundary

Review `ResolvedFile`, `ResolvedDirectory`, and `BodySource`:

- normal consumers cannot manufacture confined capabilities;
- extraction/reconstruction remains behind `python-bindings-internal`;
- extraction docs state that confinement provenance ends when raw parts are exposed;
- file-backed handler responses preserve the original handle;
- body reuse/consumption errors are explicit;
- range bodies cannot exceed their planned range.

## Workstream F — Windows decision gate

Choose and document one outcome:

1. Harden Windows using handle-relative traversal and reparse-point controls sufficient for the same security claim as Unix; or
2. Support Windows functionally but label it trusted/local-use rather than hardened.

Reflect the decision consistently in README, security policy, package metadata, platform docs, and release notes. Do not silently claim parity.

## Workstream G — Observability and error mapping

Ensure stream I/O errors:

- terminate rather than loop;
- prevent unsafe connection reuse after truncation;
- are observable through logging/metrics hooks already within scope;
- do not leak local paths to clients.

## Likely files

- `crates/eggserve-core/src/fs/*`
- `crates/eggserve-core/src/primitives/secure_root.rs`
- `crates/eggserve-core/src/primitives/body.rs`
- response streaming modules
- filesystem/path integration tests
- security and platform documentation

## Acceptance criteria

- No serving path reopens a reconstructed path after secure resolution.
- Non-regular files are rejected on supported platforms.
- Child resolution cannot escape or accept ambiguous components.
- Race/mutation tests cover the documented Unix model.
- Capability extraction is internal-feature-only and explicitly unsafe in provenance terms.
- Range/full streams remain bounded to their capability.
- Windows support level is explicit and evidence-backed.
- Client-visible errors reveal no filesystem paths.

## Non-goals

- No virtual filesystem abstraction.
- No archive serving.
- No automatic platform-equivalence claim without implementation evidence.
