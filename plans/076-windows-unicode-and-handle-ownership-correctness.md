# Plan 076 — Windows Unicode and Handle-Ownership Correctness

## Goal

Correct the Windows native-string and handle-ownership boundary before further Windows hardening work proceeds.

This plan must ensure that every native relative-open operation uses valid UTF-16 lengths, every handle wrapper has unambiguous ownership, every duplication path is fallible, and production resolver/streaming tests cover non-ASCII names and handle lifecycle behavior on Windows.

## Preconditions

- Plan 075 has pinned the corrective baseline and registered the Windows findings.
- A dedicated Windows runner is available for runtime evidence.
- Existing Windows resolver prototypes and production paths are identifiable.

## Non-goals

Do not:

- complete handle-relative directory enumeration;
- promote Windows hardened profiles;
- add broad support for network filesystems or non-NTFS filesystems;
- redesign the cross-platform filesystem API beyond what ownership correctness requires;
- add unrelated Windows platform features;
- replace runtime Windows evidence with cross-compilation only.

## Defect statement

Current `UNICODE_STRING` construction derives `Length` and `MaximumLength` from Rust UTF-8 byte length in multiple relative-open functions. Windows native string lengths are byte lengths of the UTF-16 backing buffer. Non-ASCII names can therefore be truncated, over-read, rejected, or interpreted inconsistently.

At least one helper also risks converting a borrowed raw handle into an owned wrapper, which can close a handle it does not own. Handle cloning/duplication paths must not panic in a request path.

## Track A — Inventory every unsafe Windows call

Enumerate all Windows FFI call sites in the filesystem/runtime path, including:

- `NtOpenFile` or equivalent native relative opens;
- `CreateFileW` fallbacks;
- file and directory metadata queries;
- handle duplication;
- final-path and file-identity queries;
- raw-handle to `std::fs::File` conversion;
- Tokio file conversion;
- directory enumeration prototypes.

For each call document:

- handle ownership on entry and return;
- pointer/buffer lifetime;
- string encoding and length unit;
- structure initialization requirements;
- flags and access/share semantics;
- error conversion;
- cleanup on every failure branch.

Unsafe blocks must have local safety comments stating these invariants.

## Track B — Shared native Unicode component type

Introduce one internal type or helper for native path components.

Required behavior:

- accept `&OsStr` or equivalent Windows-native path input rather than requiring UTF-8;
- encode once using `encode_wide`;
- retain a stable UTF-16 backing allocation for the duration of the native call;
- calculate byte length as `utf16_units * size_of::<u16>()`;
- use checked multiplication and checked conversion to `u16`;
- reject components whose byte length exceeds `UNICODE_STRING` capacity;
- define whether a trailing NUL is stored and exclude it from `Length` while including it in `MaximumLength` only when appropriate;
- reject embedded NUL values before the native boundary;
- expose a safe accessor that constructs the transient native structure without allowing the backing buffer to move.

The helper must be the only approved path for constructing relative-open `UNICODE_STRING` values.

## Track C — Replace affected call sites

Migrate at minimum:

- directory-relative open;
- file-relative open;
- any-type relative open;
- root/component traversal helpers;
- future child-index lookup hooks already present in the module.

Remove duplicated length arithmetic and ad hoc UTF-16 vectors.

Verify that error paths do not retain pointers to temporary buffers or use a moved vector.

## Track D — Ownership taxonomy

Define explicit internal handle categories:

- borrowed handle: never closes on drop;
- owned handle: closes exactly once on drop;
- duplicated owned handle: returned only by a fallible duplication function;
- transferred handle: ownership intentionally moved into `std::fs::File` or Tokio file exactly once.

Prefer standard-library `BorrowedHandle`/`OwnedHandle` where practical. If a project wrapper remains necessary, its constructors must make ownership explicit in the function name and safety contract.

Prohibit constructing an owned wrapper directly from a borrowed root handle in safe code.

## Track E — Fallible duplication and transfer

Replace panic-based `Clone` semantics for OS handles.

Required API properties:

- duplication returns `io::Result<OwnedHandle>` or an equivalent structured error;
- call sites propagate failure through server/configuration errors;
- no request path panics because `DuplicateHandle` fails;
- transfer into `std::fs::File` consumes the owned wrapper and prevents double-close;
- failed conversion leaves exactly one owner responsible for cleanup;
- tests can inject or simulate duplication failure where direct OS failure is difficult to trigger.

If a type cannot implement truthful infallible `Clone`, do not implement `Clone`.

## Track F — Access/share/option review

Review native open parameters used for asynchronous streaming.

Document and test:

- desired access required for metadata and reading;
- share flags needed for normal content replacement semantics;
- directory versus file create options;
- reparse-point suppression flags;
- synchronous versus asynchronous I/O flags;
- compatibility with conversion to Tokio-backed streaming;
- why the selected combination does not reproduce the recent CI regression involving `FILE_SYNCHRONOUS_IO_NONALERT`.

Do not guess. Validate full-file and range streaming on the dedicated runner.

## Track G — Unicode production-path tests

Add tests that pass through the real pinned-root resolver and response path for:

- ASCII component;
- Latin-1 accented component;
- Greek or Cyrillic component;
- CJK component;
- combining-mark sequence;
- supplementary-plane character represented by a surrogate pair;
- mixed ASCII/non-ASCII nested components;
- non-UTF-8-representable `OsStr` cases where Windows permits construction;
- longest valid component near the `UNICODE_STRING` boundary;
- overlength component rejection;
- embedded NUL rejection.

Tests must cover both files and directories. Where directory index lookup still uses a non-final path, record that limitation without claiming Release D behavior.

## Track H — Handle lifecycle tests

Add deterministic lifecycle tests for:

- borrowed root remains valid after helper returns;
- duplicated handle closes independently;
- transferring an owned handle into `File` does not double-close;
- repeated successful resolution does not grow process handle count beyond bounded noise;
- repeated not-found/denied/error paths do not leak handles;
- injected duplication failure returns an error and leaves root usable;
- failed native open cleans all temporary allocations and handles;
- resolved file continues streaming after pathname replacement according to pinned-handle semantics.

Use process handle-count evidence on Windows for integration/soak checks, while keeping unit tests deterministic.

## Track I — FFI layout and error mapping

Add compile-time or runtime assertions where practical for native structure layout and field width.

Ensure:

- NTSTATUS values are converted consistently;
- access denied, not found, invalid name, reparse denial, and overlength input map to stable internal categories;
- raw OS codes remain available for diagnostics without leaking sensitive full paths;
- no uninitialized padding or output structure is read before the native call reports success.

## Track J — Documentation and support metadata

Update:

- Windows FFI architecture note;
- unsafe-code inventory;
- threat model handle-ownership section;
- Windows known limitations;
- dedicated-runner gate documentation;
- finding registry and evidence invalidation mapping.

Do not promote Windows hardened support. State only that native Unicode and ownership primitives have been corrected.

## Required verification

On Windows x86_64, run:

- formatting and Clippy for Windows-gated code;
- unit tests for native string helper;
- production resolver Unicode corpus;
- full-file streaming;
- range streaming;
- replacement-after-open behavior;
- handle count/repeated failure tests;
- installed binary tests;
- installed Python wheel static-serving tests where Windows wheels are supported;
- release-feature matrix relevant to filesystem and streaming.

Also run Unix tests to confirm no cross-platform API regression.

## Acceptance criteria

- All relative-open native strings derive lengths from UTF-16 code units, never UTF-8 bytes.
- Components are accepted as Windows-native `OsStr` values without an unnecessary UTF-8 requirement.
- Overlength and embedded-NUL inputs fail before the native call.
- Borrowed handles cannot be converted into owned wrappers through safe APIs.
- Handle duplication is fallible and cannot panic a server request path.
- Every transferred handle has exactly one final owner.
- Non-ASCII file and directory tests pass through production resolver paths on Windows.
- Full-file and range streaming pass with the final selected native open flags.
- Repeated success and failure tests show no unbounded handle growth.
- Unsafe call sites contain specific ownership, lifetime, and initialization safety comments.
- Corrective evidence is tied to the exact implementation SHA.

## Stop conditions

Stop and document rather than masking the problem if:

- Tokio streaming requires open flags incompatible with the intended native handle model;
- a Windows path class cannot be represented safely by the chosen component API;
- a borrowed-versus-owned distinction cannot be enforced in safe Rust;
- dedicated Windows evidence is unavailable;
- fixes reveal a confinement bypass or memory-safety defect beyond this plan's scope.

Any newly discovered critical/high Windows issue must be added to the finding registry and blocks Release A.

## Handoff

Plan 077 may proceed independently after Plan 075. Release A closes only when both Plans 076 and 077 satisfy their evidence gates. The corrected handle primitives from this plan are prerequisites for the future Release D Windows directory-handle work.