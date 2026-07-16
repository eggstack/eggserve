# Phase 61 — Pinned Root Identity and Opened-Resource Ownership

## Goal

Refactor filesystem confinement so the serving root is opened once, pinned for the server lifetime, and used as the sole authority for request resolution. Ensure every production response is produced from an already validated opened file or directory object rather than from a pathname that is validated and later reopened.

This phase strengthens the Unix implementation and creates the cross-platform abstraction required by Windows handle-relative confinement. It must not introduce Windows-specific FFI beyond interfaces needed for later implementation.

## Starting state

The current Unix safe-default path already uses descriptor-relative component traversal with `statat(..., AT_SYMLINK_NOFOLLOW)` and `openat(..., O_NOFOLLOW)`. Files are opened during resolution rather than reopened by absolute path.

The configured root is currently canonicalized and opened during request resolution rather than retained once for the server lifetime. This is correct for existing confinement tests but leaves avoidable ambiguity around root replacement, process identity, and the future Windows design.

## Non-goals

Do not add:

- root hot reload or automatic root switching;
- multiple roots or virtual hosts;
- filesystem watching;
- write-capable handles;
- symlink-following hardening parity in this phase;
- Windows native traversal implementation;
- cache invalidation or content indexing beyond what is required for opened-resource ownership;
- new application-serving APIs.

## Core invariants

### Root identity

- The root is opened during server/static-service construction.
- The server retains ownership until shutdown/drop.
- Requests resolve relative to that opened root.
- Renaming or replacing the configured pathname does not redirect the running server to a different tree.
- Replacing the served tree requires deliberate reconstruction or restart.

### Resource identity

- A resolved file owns the opened file object used for metadata and streaming.
- A resolved directory owns the opened directory object used for index probing and listing.
- No production code reconstructs a path and calls `open`, `File::open`, `canonicalize`, or equivalent after validation.
- Range and conditional planning use metadata associated with the opened object.
- A file replacement after opening cannot change which object is streamed for the current response.

### Least authority

- Root and directory descriptors/handles are read/traverse only.
- File objects are read only.
- Public primitive consumers do not receive raw platform descriptors or handles unless an explicitly unstable internal API requires them.

## Track A — Inventory path reopening

Audit all Rust, CLI, and Python-backed static paths for:

- root canonicalization and opening;
- file metadata lookup;
- file reopening before streaming;
- index-file probing;
- directory listing;
- range reads;
- ETag and Last-Modified calculation;
- MIME determination;
- examples and downstream helper APIs;
- tests that reconstruct `safe_relative_components()`.

Produce a short architecture note listing every pathname-bearing type and whether it is:

- diagnostic only;
- policy input;
- safe relative display data;
- an opened-resource owner;
- forbidden for subsequent I/O.

Any path-based reopen in the serving pipeline is a blocking finding for this phase.

## Track B — Introduce a pinned root owner

Create or refactor a platform-neutral root type, for example:

- `PinnedRoot`;
- `RootHandle`;
- or an evolved `RootGuard`.

Required behavior:

- construction validates the configured root and opens it;
- construction fails if the root is missing, not a directory, inaccessible, or disallowed by policy;
- the object stores a display/canonical path only for diagnostics and policy checks;
- the object stores the platform root descriptor/handle in a private platform implementation;
- cloning uses safe reference-counted ownership or explicit duplication with documented semantics;
- request resolution borrows or clones the root authority without reopening by pathname;
- the root remains valid if its original pathname is renamed;
- shutdown/drop releases the root deterministically.

Decide whether the pinned root belongs to:

- `ServeState`;
- `StaticService`;
- `SecureRoot`;
- or a lower-level confinement type shared by all three.

Prefer one canonical owner with cheap shared references rather than independent roots created by wrappers.

## Track C — Refactor Unix traversal

Preserve the existing Unix security shape:

- start from the pinned root directory descriptor;
- inspect each component without following links;
- open intermediate components with directory and no-follow flags;
- open final components with read-only and no-follow flags;
- reject every denied link component;
- never resolve from process current working directory;
- never join an absolute root pathname for I/O.

Review descriptor duplication and lifetime carefully:

- request-local intermediate descriptors must close promptly;
- the root descriptor must not be consumed by traversal;
- cancellation and early errors must not leak descriptors;
- directory listing and index lookup must not outlive required directory authority unexpectedly;
- file streaming must retain the final file descriptor until the body completes or is cancelled.

If the platform implementation currently requires reopening `.` relative to the root, document and test that this remains descriptor-relative and identity preserving.

## Track D — Opened resource model

Refactor resolved resources so their semantics are explicit.

A resolved file should contain or own:

- opened file object;
- validated metadata snapshot or a method to query metadata from the same object;
- safe display name/relative components;
- response-planning inputs;
- no public absolute serving pathname.

A resolved directory should contain or own:

- opened directory authority;
- safe relative identity;
- policy state needed for listing/index lookup;
- no requirement to reopen the directory by path.

Evaluate whether existing `ResolvedFile`, `ResolvedDirectory`, `StaticFile`, `BodyPlan`, or internal body types accidentally expose a pathname that downstream server code can use to bypass the guarantee. Retain safe display paths only when clearly typed and documented as non-I/O data.

## Track E — Index lookup and directory listing

Index resolution must:

- use the validated directory descriptor;
- probe configured index names relative to that descriptor;
- apply no-follow and dotfile policy to index candidates;
- return an opened index file;
- not join and reopen a path.

Directory listing must:

- enumerate from the validated directory object;
- apply link and dotfile policy to every entry;
- bound entry count and output size if those limits already exist, otherwise leave limit expansion to Plan 067 but make enumeration compatible with it;
- avoid stat-by-absolute-path patterns;
- avoid leaking absolute local paths in errors or HTML.

## Track F — Server and Python integration

Ensure every static-serving entry point shares the pinned-root implementation:

- CLI binary;
- Rust `StaticService`;
- Rust `SecureRoot` primitives;
- Python `ServerSecureRoot`;
- Python native `SecureRoot`;
- subprocess server path.

Do not create one pinned root in Python and another in the Rust service for the same object unless ownership semantics require it and tests prove identity behavior.

Python wrappers must not expose raw FDs/handles or invite reopening through `open()`.

## Track G — Error taxonomy

Add or refine errors for:

- root open failure;
- root not directory;
- root identity unavailable;
- root authority invalidated unexpectedly;
- component open failure;
- denied symlink;
- final object type mismatch;
- metadata failure on opened object;
- platform unsupported hardened operation.

Client responses must remain sanitized. Internal diagnostics may include a sanitized configured root display value but should not include arbitrary request-controlled strings unsafely.

## Required tests

### Root identity tests

- start server, rename configured root pathname, verify already pinned content remains served;
- replace old pathname with a different directory, verify new tree is not served;
- delete/unlink root pathname where platform semantics permit, verify pinned open files/directories behave according to documented semantics;
- reconstruct service after replacement and verify new root is then used;
- multiple services with distinct roots remain isolated.

### Resource identity tests

- resolve file, atomically replace pathname, stream resolved file, verify original opened object is served;
- resolve file, truncate/modify through another handle, document and test expected same-object semantics;
- resolve directory, replace index pathname, verify request uses the opened index object selected during resolution;
- cancellation during streaming releases file descriptors;
- failed index probes release intermediate descriptors;
- repeated 404/403 paths do not grow descriptor count.

### Regression tests

- all traversal, percent-decoding, dotfile, symlink, directory-listing, range, conditional, HEAD, and production-path tests;
- Python native and live-server static tests;
- corpus replay and fuzz seeds;
- Unix race tests already present;
- public API snapshots.

Add Linux and macOS descriptor-count stress tests where reliable. Use platform-tolerant baselines rather than brittle exact counts.

## Release criteria changes

Add required gates for:

- pinned-root identity behavior;
- no-reopen static pipeline tests;
- root replacement regression;
- opened-file replacement regression;
- Linux and macOS execution;
- Python/Rust parity where applicable.

Invalidate these gates on changes to:

- filesystem modules;
- static service;
- response body/file streaming;
- root configuration;
- Python secure-root bindings.

## Required documentation

Update:

- filesystem architecture;
- threat model;
- secure-root documentation;
- extension contract;
- platform support notes;
- unsafe path reconstruction warning.

Document clearly:

- a running server pins root identity;
- changing the root pathname does not retarget the server;
- restart/reconstruction is required to serve a replacement root;
- safe relative components are display/policy data, not reopening authority.

## Acceptance criteria

- Root authority is opened once and retained for server lifetime.
- Unix traversal begins from the pinned descriptor.
- Every static response streams from an already validated opened file.
- Index lookup and directory listing remain descriptor-relative.
- Root/path replacement cannot redirect a running server outside its pinned tree.
- Descriptor lifetime is cancellation-safe and leak-free.
- CLI, Rust, and Python static paths share the same implementation.
- Existing security and conformance gates remain green.

## Stop conditions

Stop and document if:

- a public API requires exposing raw platform handles to preserve compatibility;
- the response planner fundamentally requires path reopening;
- a platform cannot preserve the open-once invariant without a separate support classification;
- the refactor would introduce root hot reload, multiple roots, or virtual hosting.

Do not weaken the invariant to preserve an internal pathname-oriented abstraction. Refactor the abstraction instead.

## Handoff

Plan 062 should consume the platform-neutral pinned-root and opened-resource interfaces established here. The Windows spike must prove an implementation of the same invariants rather than creating a weaker parallel path.
