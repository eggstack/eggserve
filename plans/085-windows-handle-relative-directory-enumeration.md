# Plan 085 — Windows Handle-Relative Directory Enumeration

## Goal

Replace Windows pathname-based directory enumeration with an implementation that enumerates from the already validated directory handle, preserves the deny-all-reparse policy, and keeps directory-listing behavior inside the same confinement authority as direct file and index serving.

This plan is the second phase of Release D. It depends on Plan 084 retaining owned Windows directory handles through resolution and index lookup.

## Problem statement

The current Windows implementation reconstructs a final path from a validated handle and calls path-oriented enumeration APIs. That path reconstruction is diagnostically useful but is not an acceptable source of filesystem authority for the hardened profile. It reintroduces pathname races, namespace normalization ambiguity, and a split security model between direct file serving and directory listing.

Directory listing is disabled by default and excluded from the initial hardened profile. Nevertheless, the internal implementation must be correct before the option can be qualified, and handle-relative enumeration is also useful for secure child discovery and verification.

The required invariant is:

> Every directory entry considered by the Windows hardened implementation must be obtained from the already opened directory handle, and every entry selected for later use must be opened and validated relative to that handle.

## Preconditions

- Plan 084 is closed with exact-SHA Windows evidence.
- `ResolvedDirectory` owns a Windows directory handle.
- Fallible handle duplication and UTF-16 correctness are already in place.
- Directory listing remains disabled by default and outside the hardened production profile until this plan and Plan 086 close.

## Non-goals

Do not add:

- recursive directory listing;
- templating, themes, sorting customization, pagination UI, or rich file metadata;
- directory watches or live update streams;
- filesystem writes;
- symlink/reparse following;
- SMB or non-NTFS hardened claims;
- a public raw enumeration API exposing Windows structs or handles;
- performance optimizations that weaken boundedness or validation.

## Track A — Select and document the enumeration API

Implement enumeration from a directory handle using a supported Windows mechanism such as `GetFileInformationByHandleEx` with a directory information class or a carefully wrapped native directory-query API.

Before implementation, record an ADR amendment covering:

- the selected API and Windows support floor;
- buffer layout and alignment requirements;
- restart semantics for repeated calls;
- end-of-directory signaling;
- filename encoding and length interpretation;
- file identity and attribute availability;
- cancellation/blocking behavior;
- whether enumeration is synchronous and how it is isolated from the async runtime;
- error mapping;
- why path-based `FindFirstFileW` is not used in the hardened branch.

Prefer the least complex API that provides:

- enumeration from an existing handle;
- UTF-16 filename length in bytes or code units with unambiguous documentation;
- file attributes and reparse indication;
- optional stable file identity;
- deterministic continuation through a caller-owned bounded buffer.

## Track B — Build a safe bounded parser for variable-length entries

Directory information APIs commonly return variable-length linked records. Implement parsing with explicit bounds checks.

Required checks for every buffer:

- returned byte count does not exceed allocation;
- each entry header is fully contained;
- filename length is even where expressed in bytes;
- filename range lies within the returned buffer;
- `NextEntryOffset` is zero only for the final entry;
- non-zero offsets advance and remain aligned as required;
- offsets cannot loop, underflow, or exceed the returned byte count;
- invalid UTF-16 is handled deterministically without unsafe truncation;
- no stack or fixed-size filename assumption is used;
- the parser rejects malformed kernel/API output rather than indexing unchecked.

Encapsulate unsafe FFI at the smallest boundary. The parser itself should operate on byte slices with ordinary Rust bounds checks where possible.

Add unit tests using synthetic buffers for:

- one entry;
- multiple entries;
- zero-length filename;
- odd filename byte length;
- offset before current record end;
- offset beyond buffer;
- offset loop;
- truncated header;
- truncated filename;
- unpaired surrogate;
- maximum accepted filename;
- end-of-directory with zero bytes.

## Track C — Introduce an internal platform-neutral listing record

Map Windows entries into a small internal representation, for example:

```text
DirectoryEntryRecord
  name: OsString/String
  kind: file | directory | other | reparse
  identity: optional platform identity
  hidden_or_dot: bool
```

Requirements:

- do not expose raw Windows attribute bits publicly;
- preserve enough information to apply policy before rendering;
- do not convert through lossy UTF-8 if the server otherwise supports the name;
- define behavior for names that cannot be represented in a URL safely;
- skip `.` and `..` pseudo-entries if returned;
- classify all reparse points as denied in the hardened branch regardless of tag;
- reject or omit unsupported object classes such as devices.

If the public directory-listing renderer requires UTF-8 strings, define one explicit policy for non-Unicode or invalid names: omit with a categorized event, percent-encode a reversible representation, or fail the listing safely. Do not silently produce ambiguous replacement-character names that could refer to a different entry.

## Track D — Apply policy before rendering

The listing pipeline must apply the same policy as direct resolution:

- dotfiles denied unless explicitly enabled;
- reparse points denied and omitted;
- unsupported object classes omitted;
- names that violate Windows component policy omitted;
- listing entry count bounded;
- generated output bytes bounded;
- HTML escaping mandatory;
- URL path segment encoding performed exactly once;
- no absolute local path exposed.

Add explicit limits to the authoritative configuration model if Plans 075–083 did not already define them:

- maximum entries enumerated;
- maximum listing response bytes;
- maximum single encoded filename bytes;
- enumeration timeout or blocking-work budget.

Zero and out-of-range values must fail configuration validation rather than creating unbounded or zero-capacity behavior.

## Track E — Reopen selected entries relative to the directory handle

Enumeration metadata is not sufficient authority for serving a file. If any listing or future internal operation selects an entry after enumeration:

1. validate the name as one component;
2. open it relative to the retained directory handle;
3. re-query type and reparse metadata from the opened handle;
4. reject if identity/type changed incompatibly;
5. serve only from the opened final handle.

This closes enumeration-to-open races. Never call `File::open(entry_path)` or equivalent on a rendered/listed entry.

Tests should swap an entry between enumeration and open:

- regular file to reparse point;
- directory to junction;
- file to directory;
- same-name replacement file;
- delete and recreate;
- access permission change.

Safe outcomes are either the newly opened, policy-valid object or a denial/not-found response. Bytes outside the pinned root must never be served.

## Track F — Async runtime integration

Directory enumeration may use synchronous Windows APIs. Integrate it without blocking core async executor threads indefinitely.

Requirements:

- use a bounded blocking-work mechanism if enumeration can block;
- cap concurrent listing operations separately or under the authoritative file-operation limit;
- cancellation/shutdown prevents new work and stops waiting according to the lifecycle contract;
- a blocked OS call cannot cause the server to claim all work has stopped unless the support contract explicitly accounts for it;
- timeouts are named according to actual semantics;
- permits return on success, error, cancellation, and client disconnect.

Do not introduce one thread per entry or one unbounded task per listing.

## Track G — Replace path-based fallback in hardened mode

Remove hardened-mode reachability of:

- `GetFinalPathNameByHandleW` followed by wildcard construction;
- `FindFirstFileW`/`FindNextFileW` for authority;
- `std::fs::read_dir` on reconstructed paths;
- canonical-path enumeration.

Those helpers may remain only for explicitly functional/compatibility profiles, with separate names and documentation.

Add tests or source checks proving that Windows hardened listing uses the handle-based implementation. Keep `GetFinalPathNameByHandleW` only for diagnostics and defense-in-depth identity checks, never as the path to enumerate.

## Track H — Listing HTTP correctness

Run listing responses through the canonical response normalization path delivered by Plans 081–083.

Required behavior:

- GET returns a bounded HTML representation;
- HEAD returns the same status and representation headers with no body;
- errors are normalized identically for GET and HEAD;
- `Content-Type` includes an explicit safe charset;
- `X-Content-Type-Options: nosniff` is retained;
- generated `Content-Length` is exact when known;
- client disconnect releases enumeration/rendering resources;
- no range support is implied for generated listing bodies unless deliberately supported by the canonical planner;
- conditional semantics are either deterministic or explicitly unsupported for generated listings.

## Track I — Windows filesystem and name matrix

Exercise enumeration on local NTFS with:

- empty directory;
- many entries;
- long names;
- non-ASCII and surrogate-pair names;
- case-colliding names where Windows permits them;
- hidden/system attributes;
- read-only files;
- directories;
- file and directory symlinks;
- junctions and mount points;
- unknown/custom reparse tags where fixture creation is possible;
- cloud placeholder entries, classified outside the hardened profile;
- concurrent rename/delete/create churn.

Record fixture-creation capability separately from test pass status. A skipped reparse test must not count as evidence that denial works.

## Track J — Resource and leak qualification

Measure handle, task, and memory behavior for:

- 10,000 repeated small listings;
- listing at entry-count limit;
- listing exceeding entry-count limit;
- client disconnect during rendering;
- repeated denied reparse entries;
- malformed or access-denied entries;
- graceful shutdown during listing;
- forced shutdown during listing;
- concurrent listings at the configured limit.

Acceptance thresholds should be baseline-relative and account for allocator noise, but sustained handle growth or permit loss is a failure.

## Required tests

At minimum:

- parser fuzz/property tests for variable-length directory buffers;
- production handle-based enumeration on Windows;
- no path fallback in hardened profile;
- reparse entries omitted/denied;
- dotfiles omitted by default;
- HTML and URL escaping corpus;
- GET/HEAD listing parity;
- entry and output-byte limits;
- enumeration-to-open swap race;
- cancellation and shutdown;
- repeated listing handle-count stability;
- installed Windows binary and wheel listing smoke tests.

## Fuzzing

Add a deterministic fuzz target for the pure directory-buffer parser. Seed it with:

- valid single and multi-entry buffers;
- malformed offsets;
- truncated records;
- odd UTF-16 lengths;
- invalid surrogate sequences;
- maximum-length names;
- zero-byte and random buffers.

The target must assert no panic, no out-of-bounds access, no infinite loop, and bounded allocation.

## Release-gate changes

Add gates such as:

- `windows.directory-enumeration-handle-relative`;
- `windows.directory-buffer-parser`;
- `windows.directory-listing-policy`;
- `windows.directory-listing-head-parity`;
- `windows.directory-listing-resource-stability`;
- `windows.directory-listing-installed-artifact`.

These gates are required only for profiles that enable or rely on directory enumeration. Windows profile promotion still remains blocked until Plan 086.

## Documentation changes

Update:

- Windows filesystem ADR;
- filesystem confinement architecture;
- directory-listing security documentation;
- configuration reference for listing limits;
- threat model;
- release contract;
- support-profile exclusions;
- deployment guide warning that listing remains opt-in.

Document the exact non-UTF-8 filename behavior and the fact that enumeration metadata is never used as authority to reopen by path.

## Acceptance criteria

- Windows hardened enumeration starts from a retained directory handle.
- Variable-length records are parsed with explicit bounds checks.
- Reparse and disallowed entries are filtered before rendering.
- Selected entries are reopened and validated relative to the same directory handle.
- Hardened mode cannot reach path-based enumeration.
- Listing work and output are bounded.
- GET/HEAD and error behavior use canonical normalization.
- Cancellation and shutdown return handles/tasks/permits to baseline.
- Installed Windows artifacts execute the same handle-based path.
- Exact-SHA dedicated Windows evidence passes.

## Stop conditions

Stop rather than weakening the profile if:

- the enumeration API cannot be used safely from a retained handle;
- record layouts cannot be validated against the Windows SDK and real runners;
- non-UTF-8 name behavior cannot be made unambiguous;
- cancellation semantics make claimed shutdown completion false;
- listing requires path reconstruction for authority.

## Handoff

After this plan closes, Plan 086 may perform end-to-end adversarial qualification and decide whether the Windows reverse-proxy profile can be promoted.