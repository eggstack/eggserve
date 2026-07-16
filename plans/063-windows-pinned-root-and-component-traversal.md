# Phase 63 — Windows Pinned Root and Component-Relative Traversal

## Goal

Implement the production Windows confinement core approved by Plan 062: a pinned root directory handle, component-by-component handle-relative traversal, deny-all reparse policy, stable root identity, and final opened-resource ownership.

This phase owns filesystem resolution only. File response integration, index lookup, and directory listing are completed in Plan 064.

## Preconditions

- Plan 061 pinned-root interfaces are merged.
- Plan 062 has a positive architecture decision backed by real Windows evidence.
- The approved API and dependency choice are recorded.
- Windows remains `functional` or `candidate`, not `supported-hardened`.

## Non-goals

Do not add:

- reparse-following mode to the hardened resolver;
- support claims for SMB, ReFS, FAT, cloud placeholders, or third-party filesystems;
- path-based canonicalization as the primary security mechanism;
- directory listing integration;
- virtual roots, multiple roots, or hot reload;
- public raw-handle APIs;
- ASGI/WSGI or application-server behavior.

## Security invariant

> Every Windows request path in the hardened profile is resolved from the retained root handle through validated relative component opens. Any reparse-point component is denied, and the final opened object is the only object eligible for later serving.

## Track A — Windows platform module

Create a narrow internal module under the filesystem platform layer. Separate:

- safe owned-handle wrappers;
- raw FFI bindings;
- root opening and identity;
- relative component opening;
- attribute/tag inspection;
- error conversion;
- test-only helpers.

Keep unsafe operations small and locally auditable. Every unsafe block must document:

- structure initialization;
- buffer lifetime;
- handle ownership;
- access/share flags;
- pointer and length validity;
- thread-safety assumptions;
- conversion to safe Rust types.

No raw Windows type should appear in stable public APIs.

## Track B — Pinned Windows root

At root construction:

- validate the configured path through existing configuration policy;
- open the root as a directory with read/traverse authority only;
- suppress unintended reparse traversal;
- reject the root if it is itself a reparse point;
- query filesystem type;
- query volume serial and file identity;
- query final handle path for diagnostics/defense in depth;
- retain the owned handle for root lifetime;
- store only sanitized display data outside the platform object.

Initial hardened eligibility:

- local volume;
- NTFS;
- ordinary directory;
- no reparse attribute;
- APIs needed by the approved design available.

For unsupported roots, return a typed classification error or construct only the explicitly functional fallback when the caller opted into functional behavior. Do not silently claim hardened mode.

## Track C — Component validation boundary

The Windows resolver consumes `ConfinedPath` components, not raw request strings.

Before each platform open, assert or validate:

- non-empty component;
- not `.` or `..`;
- no slash or backslash;
- no NUL;
- no colon or ADS syntax;
- no drive prefix;
- not a reserved device name, including extension variants;
- no trailing dot or space alias;
- component length within Windows and eggserve limits.

Do not perform transformations that map an unsafe input to a safe-looking component. Reject ambiguous names.

## Track D — Relative component open

For each component:

1. Use the current directory handle as the root for the next open.
2. Open the named object without following reparse behavior.
3. Query attributes and reparse tag from the opened object.
4. Reject any reparse attribute regardless of tag.
5. For intermediate components, require directory type.
6. For the final component, classify file or directory.
7. Retain the opened final handle.
8. Close intermediate handles promptly as traversal advances.

Error behavior must distinguish:

- not found;
- access denied;
- reparse denied;
- intermediate not directory;
- unsupported object type;
- invalid component;
- platform/API failure;
- root authority failure.

Map client-visible behavior consistently with Unix without exposing local paths.

## Track E — Defense-in-depth identity checks

After the final open:

- query final path from handle;
- query volume and file identity;
- ensure volume matches the pinned root for the initial hardened profile;
- reject unexpected namespace/device results;
- record identity internally for race tests and diagnostics;
- do not use string-prefix comparison as the primary confinement decision.

The handle-relative no-reparse traversal is authoritative. Final-path checking is a secondary detection layer.

## Track F — Resolved resource integration

Extend the platform-neutral resolved-resource model so Windows final handles become owned file/directory resources.

Requirements:

- no absolute child path required for later I/O;
- final handle ownership is unique or safely reference counted;
- metadata can be queried from the handle;
- resources are `Send`/`Sync` only when the underlying semantics justify it;
- cancellation and drop close handles;
- Python wrappers cannot extract raw handles;
- safe relative/display names remain non-authoritative.

Leave response body conversion behind a narrow internal method for Plan 064 if not completed here.

## Track G — Functional fallback separation

If a legacy non-hardened Windows fallback remains:

- give it an explicit internal mode/status;
- prevent accidental selection by a hardened profile;
- emit a clear startup/support classification;
- keep release criteria from counting fallback tests as hardened evidence;
- document exactly which roots trigger fallback or rejection.

Prefer fail-closed for public/hardened profile construction rather than silently downgrading.

## Required tests

### Root tests

- ordinary local NTFS root opens;
- file path as root rejected;
- root symlink rejected;
- root junction rejected;
- inaccessible root rejected;
- root rename does not change retained identity;
- replacement directory at old pathname is not served;
- unsupported filesystem classification is deterministic.

### Traversal tests

- direct and nested regular files;
- nested directories;
- missing component;
- intermediate file;
- final directory;
- file symlink at each depth;
- directory symlink at each depth;
- junction at each depth;
- mount-point reparse where available;
- unknown reparse tag where available;
- reserved names and extension variants;
- ADS syntax;
- trailing dot/space;
- mixed case;
- long names within limits;
- concurrent ordinary/reparse replacement.

### Resource tests

- final handle identity recorded;
- final pathname replaced after resolution does not change object;
- repeated failures do not leak handles;
- cancellation/drop releases all request-local handles;
- parallel requests remain root-isolated.

## Fuzz and property coverage

Add platform-independent property tests for Windows component rejection. Add Windows-only generated tests that combine validated components and race replacement where deterministic.

Seed corpus with:

- device namespace prefixes;
- UNC-like forms;
- reserved names;
- ADS forms;
- trailing normalization aliases;
- encoded separators;
- reparse test fixtures identified by tag.

## Release criteria

Add candidate gates for:

- Windows pinned-root identity;
- relative traversal;
- deny-all reparse behavior;
- no pathname reopen;
- handle leak checks;
- dedicated-runner privileged reparse cases.

These gates do not yet promote Windows. Promotion belongs to Plan 065.

## Documentation

Update the Windows filesystem architecture and threat model with:

- supported API design;
- deny-all reparse policy;
- NTFS/local-volume candidate restriction;
- fallback behavior;
- root identity behavior;
- raw-handle non-exposure;
- remaining unqualified operations owned by Plan 064/065.

## Acceptance criteria

- Hardened Windows root creation pins an ordinary local NTFS directory handle.
- Every component opens relative to the previous validated directory handle.
- Every reparse-point component is denied.
- Final resources own validated handles.
- No Windows hardened path performs child I/O by reconstructed absolute pathname.
- Root rename/replacement cannot retarget the running resolver.
- Errors are typed and sanitized.
- Handle lifetime is leak-free under errors, races, and cancellation.

## Stop conditions

Stop and retain Windows functional-only status if:

- integration diverges from the approved Plan 062 architecture;
- any component must be reopened by absolute path;
- reparse detection occurs only after target bytes are used;
- unsupported filesystems silently enter hardened mode;
- unsafe ownership cannot be proven.

## Handoff

Plan 064 must consume the final opened file/directory resources created here. It must not bypass this resolver for index files, listings, metadata, ranges, or streaming.
