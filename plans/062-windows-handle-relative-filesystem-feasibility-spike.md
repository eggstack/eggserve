# Phase 62 — Windows Handle-Relative Filesystem Feasibility Spike

## Goal

Prove, in isolated Windows-only code, that eggserve can implement the same open-once confinement invariant on Windows that it provides on Unix: a pinned root directory handle, component-by-component relative opening, deny-all reparse-point policy, and final serving from the validated handle.

This is a feasibility and architecture phase. It should not prematurely replace the functional Windows fallback or mark Windows hardened.

## Starting state

After Plan 061, eggserve should have:

- a platform-neutral pinned-root abstraction;
- opened-resource ownership in the static-serving pipeline;
- Unix descriptor-relative traversal from a retained root descriptor;
- tests proving root and resource identity behavior.

Windows currently has parser-level rejection for reserved names, drive prefixes, ADS syntax, and backslash ambiguity, but filesystem resolution uses a weaker metadata/canonicalization fallback without robust reparse-point race resistance.

## Decision to prove

The preferred Windows design is:

1. Open and retain the root directory handle.
2. Resolve each request component relative to the current directory handle.
3. Suppress ordinary reparse-point traversal.
4. Query attributes and reparse tag from the opened object.
5. Reject any reparse-point object under the hardened profile.
6. Require intermediate objects to be directories.
7. Retain the final opened file/directory handle.
8. Perform metadata, listing, index lookup, and streaming using that handle.

The spike must determine whether this can be implemented safely and maintainably using supported Windows APIs and Rust ownership patterns.

## Non-goals

Do not:

- promote Windows support status;
- implement the complete production resolver;
- add reparse-following support;
- support SMB, ReFS, FAT, cloud placeholders, or third-party filesystems as hardened;
- introduce virtual roots, multiple roots, or hot reload;
- expose raw handles publicly;
- add application-server behavior;
- create a broad Windows abstraction crate inside eggserve.

## Track A — API research and architecture record

Evaluate the smallest viable implementation using:

- `NtCreateFile` or `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory` for root-relative opens;
- `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT` where appropriate;
- `GetFileInformationByHandleEx` with `FileAttributeTagInfo`, `FileIdInfo`, and directory information classes;
- `GetFinalPathNameByHandleW` for defense-in-depth diagnostics/verification;
- safe Rust wrappers from `windows-sys`, `windows`, or another maintained crate;
- minimal local FFI only where stable crate coverage is insufficient.

Produce an architecture decision record covering:

- API choice and fallback hierarchy;
- minimum supported Windows version;
- whether `ntdll` calls are statically imported or dynamically resolved;
- exact desired access, share, disposition, and option flags;
- `HANDLE` ownership and duplication rules;
- `NTSTATUS`/Win32 error conversion;
- directory versus file open semantics;
- cancellation and blocking behavior;
- Tokio integration;
- compatibility with CPython wheel builds;
- security implications of undocumented or semi-documented NT APIs;
- testability on GitHub-hosted and dedicated Windows runners.

The ADR must include a go/no-go conclusion and list any unsupported environment detected by the spike.

## Track B — Root-relative open prototype

Implement a Windows-only internal prototype that:

- opens a temporary root directory;
- retains the root handle;
- opens a direct child relative to the root handle;
- opens a nested child by iteratively replacing the current directory handle;
- opens both files and directories;
- rejects missing components and type mismatches;
- never constructs an absolute child path for opening;
- returns an owned final handle.

Use fixed, validated UTF-16 component names. The prototype must reject components containing:

- separators;
- NUL;
- colon/ADS syntax;
- drive prefixes;
- dot and dot-dot;
- reserved names;
- trailing dot/space aliases according to the planned policy.

The component parser remains the existing cross-platform parser. The spike only consumes already validated components.

## Track C — Reparse suppression and inspection

Prove behavior for:

- file symbolic links;
- directory symbolic links;
- junctions;
- mount points if the runner permits creation;
- unknown/custom reparse tags where a test helper can create them;
- final and intermediate reparse components.

For each object, verify:

- the open does not silently traverse to the target;
- attributes indicate reparse status;
- the tag can be retrieved;
- the prototype rejects the object before using it as a directory parent or file body;
- the target bytes are never read.

Adopt a deny-all initial rule:

> Under the hardened Windows profile, any object with `FILE_ATTRIBUTE_REPARSE_POINT` is denied regardless of tag.

Do not attempt an allowlist in this phase.

## Track D — File identity and root identity

Prove retrieval of:

- volume serial number;
- stable file identifier;
- directory/file attributes;
- final normalized handle path for diagnostics.

Test:

- root pathname rename after handle open;
- replacing the old pathname with another directory;
- comparing identity before and after rename;
- comparing separately opened handles to the same object;
- detecting a different root object at the same pathname.

The prototype should show that the retained root handle remains authoritative after pathname replacement.

## Track E — Streaming compatibility

Prove conversion from the final validated handle to the existing Rust file-streaming path.

Requirements:

- no child pathname reopen;
- handle ownership transferred or duplicated exactly once;
- asynchronous file reads work under Tokio;
- range/seek operations work;
- cancellation closes the handle;
- repeated test loops do not grow handle count;
- Python wheel/server builds can compile the implementation behind `cfg(windows)`.

If Tokio requires conversion through `std::fs::File`, document ownership semantics and ensure the raw handle is not closed twice.

## Track F — Directory enumeration feasibility

Prototype enumeration from an already opened directory handle using a handle-based API.

Prove:

- entry names can be retrieved without reopening the directory by path;
- file/directory/reparse attributes are available or can be queried relative to the directory handle;
- reparse entries can be hidden/denied;
- dotfile policy can be applied;
- enumeration can be bounded and cancelled;
- no absolute path is required.

Do not integrate listing into the production service yet. Plan 064 owns that work.

## Track G — Race probes

Create focused race tests that repeatedly swap a direct child between:

- regular file and symlink;
- directory and junction;
- file A and file B;
- ordinary directory and reparse directory.

The prototype passes if each operation either:

- opens the intended ordinary object and records its identity; or
- fails/rejects safely.

It fails if it reads through a denied reparse point or returns an object outside the root.

These probes are evidence for feasibility only, not final qualification.

## Track H — Dependency and audit policy

Any new dependency must be justified in `docs/dependency-policy.md`.

Prefer:

- one existing Windows bindings dependency already present transitively or directly;
- minimal feature selection;
- no broad Windows convenience framework;
- no unsafe abstraction without local tests and safety comments.

For every `unsafe` block:

- document pointer/length validity;
- handle ownership;
- structure initialization;
- Unicode buffer lifetime;
- error conversion;
- thread-safety assumptions;
- why safe wrappers are unavailable or insufficient.

Add a Windows FFI safety review checklist.

## Required tests

Run on a real Windows host:

- relative direct file open;
- relative nested file open;
- intermediate directory validation;
- final directory open;
- root rename and pathname replacement;
- file ID retrieval;
- file and directory symlink rejection;
- junction rejection;
- final and intermediate reparse rejection;
- range read from validated handle;
- cancellation/handle release;
- handle-based enumeration;
- concurrent replacement probes;
- CPython extension and standalone binary compilation.

Where privilege or Developer Mode is required, tests must report `skipped-with-reason` in generic CI and run as required gates on the dedicated Windows environment.

## Deliverables

- Windows-only prototype module or test crate;
- architecture decision record;
- API/flag table;
- safety commentary for FFI;
- test helpers for symlink/junction creation;
- feasibility evidence from a real Windows run;
- explicit list of unsupported filesystems/environments;
- go/no-go recommendation for Plan 063.

## Acceptance criteria

Proceed to Plan 063 only if:

- child components can be opened relative to a pinned root handle;
- ordinary reparse traversal can be suppressed;
- all tested reparse objects can be detected and denied;
- final handles can be streamed without pathname reopening;
- directory enumeration is possible from a validated directory handle;
- root identity remains stable across pathname rename/replacement;
- ownership and error semantics can be wrapped safely in Rust;
- no broad or unauditable dependency is required.

## Stop conditions

Stop and document a no-go if:

- the implementation requires pathname canonicalization as the primary confinement mechanism;
- reparse points cannot be reliably detected before target use;
- the final file must be reopened by name for streaming;
- handle ownership cannot be made unambiguous;
- the APIs required are unavailable on the declared Windows baseline;
- required FFI is too broad to audit within eggserve’s scope.

A no-go does not justify weakening the Windows production claim. Windows must remain functional-only until an alternative hardened design is proven.

## Handoff

Plan 063 should implement only the approved architecture from the ADR. Prototype-only code should either be promoted into a reviewed internal platform module or removed after its tests are transferred.
