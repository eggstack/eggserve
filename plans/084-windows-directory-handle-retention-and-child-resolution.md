# Plan 084 — Windows Directory-Handle Retention and Child Resolution

## Goal

Complete the Windows hardened filesystem path from the pinned root through directory resources, index-file lookup, and final child opening without reconstructing or reopening an absolute pathname.

This plan begins Release D of the corrective roadmap. It assumes Plans 075–083 have corrected the runtime, body, configuration, and HTTP response contracts. It must not promote a Windows production profile by itself; promotion remains blocked until Plans 085 and 086 complete qualification.

## Problem statement

The current Windows resolver can open direct file requests component-by-component relative to a pinned root handle, but directory resources lose the opened directory handle. `ResolvedDirectory` retains a descriptor only on Unix. On Windows, child lookup falls back to component reconstruction and path-based resolution. This affects ordinary static-serving behavior because `/directory/` commonly resolves `index.html` as a child of the already validated directory.

The hardened invariant is:

> Once a request enters the Windows hardened resolver, every intermediate directory, final file, directory index, and listing operation must derive from the pinned root and retained directory handles. No hardened operation may regain authority by reconstructing an absolute child path.

## Preconditions

- Plans 075–083 are closed with exact-SHA evidence.
- Plan 076 has corrected UTF-16 length construction, handle ownership, duplication error handling, and borrowed-versus-owned hazards.
- The current Windows support profiles remain `candidate` or `functional`; none are promoted during this plan.
- A dedicated Windows x86_64 runner with NTFS and permission to create symbolic links or junctions is available for the qualification subset.

## Non-goals

Do not add:

- symlink or reparse-point following in the hardened profile;
- SMB, ReFS, FAT, cloud-placeholder, or third-party filesystem guarantees;
- uploads, writes, mutation APIs, or deployment swapping APIs;
- path-based fallback inside the Windows hardened branch;
- application routing, ASGI/WSGI behavior, middleware, or proxy features;
- directory enumeration beyond the minimum interface needed by Plan 085;
- public exposure of raw Windows handles.

## Track A — Define an owned Windows directory resource

Extend the internal resolved-resource model so a Windows directory retains an owned handle.

Required shape:

- `ResolvedDirectory` contains a Windows-only owned directory handle.
- The handle type is non-public and has explicit ownership semantics.
- Cloning is either removed or performed through a fallible duplication method.
- Moving a `ResolvedDirectory` transfers ownership exactly once.
- Debug output never prints raw handle values in normal logs.
- The canonical/final path remains diagnostic only and is not used for child access.
- Safe relative components remain metadata for MIME, URL, and diagnostics only.

Preferred design:

```text
ResolvedDirectory
  canonical_path: PathBuf              # diagnostics only
  components: Vec<String>              # logical relative identity
  unix.dir_fd: File                    # cfg(unix)
  windows.dir_handle: OwnedHandle      # cfg(windows)
```

If one common platform-neutral wrapper is introduced, it must not expose platform-specific APIs publicly or force path reopening on any platform.

## Track B — Make handle duplication fallible

Directory child lookup may require retaining the parent while returning a child. Handle duplication must not panic.

Requirements:

- Replace panic-capable `Clone` behavior for Windows handles with `try_clone()` or equivalent.
- Do not implement `Clone` for an owning handle if failure cannot be represented.
- Propagate `DuplicateHandle` errors through a typed internal error.
- Ensure invalid/null handles cannot be wrapped as valid owners.
- Add explicit constructors for owned handles returned by successful Windows APIs.
- Never construct an owner from a borrowed raw root handle merely to duplicate it.
- Add debug assertions and tests for exactly-once close behavior.

Acceptance:

- handle-quota or injected duplication failure returns an error;
- no resource path panics because duplication failed;
- borrowed handles are never closed by an owning wrapper.

## Track C — Implement Windows child resolution from a retained directory handle

Add a Windows branch to `RootGuard::resolve_child()` that uses the retained directory handle.

Required sequence:

1. Validate the child as exactly one component.
2. Reject empty, `.`, `..`, NUL, slash, backslash, drive, ADS, reserved-device, trailing-dot, and trailing-space ambiguity according to the existing Windows policy.
3. Convert the child to UTF-16 and derive `UNICODE_STRING` lengths from UTF-16 code units.
4. Open relative to the retained directory handle with the appropriate file/directory options.
5. Query type and reparse metadata from the opened handle.
6. Reject every reparse point under the hardened policy.
7. Return a `ResolvedFile` or `ResolvedDirectory` retaining the opened object.
8. Do not call `canonicalize`, `File::open(path)`, `read_dir(path)`, `FindFirstFileW(path)`, or any other path-authority operation.

The child resolver must support both:

- index-file lookup where the expected result is a regular file;
- nested directory traversal required by internal resolution tests.

Error mapping must preserve the distinction among:

- not found;
- access denied;
- not a directory;
- file is a directory;
- reparse point denied;
- invalid component;
- transient sharing violation;
- internal/FFI failure.

Do not collapse security-policy rejection into a generic 404 inside the internal API. The public static-response mapping may deliberately avoid revealing details, but tests and observability need a stable category.

## Track D — Preserve handles through index lookup

Audit the static service’s directory path.

Required invariant:

> `/dir/` and an index candidate beneath `/dir/` must be resolved from the already opened `/dir/` handle.

Actions:

- ensure the directory response path calls the Windows handle-relative child resolver;
- remove or bypass fallback path reconstruction for Windows hardened mode;
- resolve every configured index candidate separately from the retained handle;
- reject index candidates that are reparse points;
- stream the selected index directly from the validated final file handle;
- preserve the unified response planner introduced by Plans 081–083;
- ensure conditionals, ranges, validators, GET, and HEAD behavior remain identical to direct-file access.

Tests must compare:

- `/dir/index.html`;
- `/dir/` selecting `index.html`;
- `/dir` redirect behavior, if supported;
- missing index;
- index is directory;
- index is symlink/junction/reparse point;
- index swapped during lookup;
- same file accessed by direct and index forms.

## Track E — Root and directory lifetime rules

Document and test lifetime semantics.

Required behavior:

- the pinned root remains open until all server-owned directory/file resources are dropped;
- a resolved directory owns its handle independently of temporary traversal handles;
- parent handles remain alive for every relative open that uses them;
- dropping an intermediate handle cannot invalidate the final opened file;
- server shutdown waits for active file streams to release their handles according to the corrected lifecycle contract;
- no directory handle is retained indefinitely after request completion unless owned by a live stream/resource.

Add resource-accounting tests that measure process handle count across:

- repeated directory requests;
- repeated index lookup;
- missing child lookup;
- denied reparse child;
- client disconnect during index streaming;
- graceful shutdown;
- forced shutdown.

## Track F — Windows Unicode and namespace coverage

Run child lookup through names containing:

- BMP non-ASCII characters;
- surrogate-pair characters;
- combining sequences;
- case variants;
- names near the component length limit;
- names whose UTF-8 byte length differs materially from UTF-16 code-unit length.

Continue rejecting namespace ambiguity:

- `C:` and drive-qualified forms;
- UNC/device prefixes;
- ADS syntax;
- reserved DOS device names with or without extensions;
- trailing spaces and dots;
- separators encoded or literal inside a component.

Tests must exercise the production `RootGuard` and static-service path, not only isolated FFI helpers.

## Track G — Remove hardened fallback reachability

Add structural and runtime checks proving Windows hardened mode cannot reach pathname fallback for:

- direct file resolution;
- directory resolution;
- index lookup;
- child lookup.

Recommended enforcement:

- split hardened and fallback implementations into clearly separate functions;
- mark path-based helpers with documentation identifying permitted profiles;
- add an internal test hook/counter that fails if fallback is entered during hardened-profile tests;
- add source-level checks for prohibited path reopen calls in the Windows hardened module where practical.

The compatibility/link-following profile may continue using a weaker path-based implementation, but the profile and startup diagnostics must state that it is outside the hardened guarantee.

## Track H — Error and observability contract

Add stable internal event categories for Windows resolution without leaking local paths:

- root-open failure;
- child-open not found;
- child access denied;
- reparse denied;
- invalid namespace/component;
- handle duplication failure;
- metadata query failure;
- unexpected Windows status/error code.

Logs must include only sanitized logical request information and numeric/categorized platform errors where needed. Absolute filesystem paths and raw handles must not appear in request logs.

## Required tests

At minimum add:

- retained directory handle survives parent pathname replacement;
- direct file remains streamable after ancestor rename where Windows semantics permit;
- index lookup uses retained directory authority;
- index reparse point is denied;
- intermediate reparse point is denied;
- final directory reparse point is denied;
- non-ASCII child names resolve correctly;
- duplicate-handle failure is typed and non-panicking;
- repeated child resolution returns handle count to baseline;
- hardened branch never enters path fallback;
- direct/index conditional and range parity remains green;
- Rust, CLI, and installed Windows wheel paths use the same resolver.

## Release-gate changes

Add required gates such as:

- `windows.handle-retained-directory`;
- `windows.handle-relative-child`;
- `windows.index-handle-relative`;
- `windows.unicode-child-production-path`;
- `windows.handle-ownership`;
- `windows.hardened-no-fallback`.

Map invalidation to:

- `crates/eggserve-core/src/fs/**`;
- static-service response path changes;
- Windows FFI wrappers;
- Windows wheel/native extension changes;
- configuration changes affecting symlink/reparse policy.

Evidence must record exact source SHA, Windows version/build, filesystem type, runner privilege/Developer Mode status, and whether link creation cases were executed or explicitly blocked.

## Documentation changes

Update:

- `architecture/adr-002-windows-handle-relative-filesystem.md`;
- `architecture/filesystem-confinement.md`;
- `architecture/security-model.md`;
- `docs/secure-root.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `docs/release-contract.md`;
- `release/support-profiles.toml` notes without promoting status.

Remove wording that calls the Windows module merely a feasibility prototype only after the production path actually uses the retained-handle implementation. Keep explicit limitations for enumeration until Plan 085 closes.

## Acceptance criteria

- Windows `ResolvedDirectory` retains an owned directory handle.
- Hardened Windows child and index resolution are handle-relative.
- No child/index request in hardened mode reconstructs filesystem authority from a path.
- Every opened component is checked for reparse status before use.
- Non-ASCII names use correct UTF-16 lengths through the production path.
- Handle duplication is fallible and non-panicking.
- Borrowed handles are never closed by owners.
- Direct and index representations retain HTTP semantic parity.
- Dedicated Windows evidence passes at the implementation SHA.
- Windows support profiles remain unpromoted pending Plans 085 and 086.

## Stop conditions

Stop and document rather than weakening the guarantee if:

- the selected Windows API cannot provide relative child opens on a supported Windows version;
- a necessary operation requires reopening by absolute path;
- the test environment cannot distinguish reparse denial from inability to create test fixtures;
- handle ownership cannot be represented without panic or ambiguity;
- directory index behavior cannot share the unified response planner.

## Handoff

After this plan closes, Plan 085 may implement directory enumeration from retained handles. Plan 086 remains blocked until both child resolution and enumeration are complete.