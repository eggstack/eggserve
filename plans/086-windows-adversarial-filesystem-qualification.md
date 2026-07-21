# Plan 086 — Windows Adversarial Filesystem Qualification

## Goal

Qualify the complete Windows hardened static-serving path against reparse points, path races, namespace ambiguity, filesystem differences, resource exhaustion, and installed-artifact behavior, then make an evidence-backed decision on whether the `windows-reverse-proxy` profile may be promoted.

This plan closes Release D. It does not automatically promote Windows direct HTTPS; that profile also depends on the internet-facing and TLS qualification in Plans 087–089.

## Preconditions

- Plans 075–085 are closed.
- Windows UTF-16 and handle ownership defects are corrected.
- Direct files, directories, index candidates, and listing enumeration remain handle-relative from the pinned root.
- The response planner and runtime lifecycle contracts from Plans 077–083 are stable.
- Dedicated Windows test environments are available in addition to generic GitHub-hosted runners.

## Non-goals

Do not broaden hardened support to:

- SMB shares;
- ReFS, FAT/exFAT, cloud-placeholder roots, or third-party filesystems;
- `--follow-symlinks` or reparse following;
- writable roots controlled by an adversarial local administrator;
- kernel, filter-driver, antivirus, or filesystem compromise;
- Windows authentication, ACL management, or impersonation;
- application serving or reverse proxying.

## Track A — Establish the Windows qualification environments

Use at least two evidence classes:

1. Standard CI runner for compile, unit, integration, ordinary NTFS, wheel, and portable tests.
2. Dedicated disposable Windows VM for Developer Mode/link privileges, junctions, mount points, race harnesses, long-running tests, and system-level resource measurements.

Record for each environment:

- Windows edition, version, and build;
- architecture;
- filesystem and volume flags;
- Developer Mode and symlink privilege state;
- administrator/non-administrator context;
- Rust/Python toolchain;
- antivirus or filter-driver conditions that could affect timing;
- source SHA and artifact hashes.

Tests that cannot create a fixture must report `blocked-fixture`, not pass or skip silently.

## Track B — Reparse-point denial matrix

Create and test, where supported:

- file symbolic links;
- directory symbolic links;
- junctions;
- volume mount points;
- nested reparse chains;
- final-object reparse points;
- intermediate-component reparse points;
- reparse root directory;
- index-file reparse point;
- listing entries that are reparse points;
- dangling reparse points;
- reparse targets inside the root;
- reparse targets outside the root;
- unknown/custom reparse tags where a safe fixture can be created;
- cloud placeholder tags, classified rather than claimed hardened.

The hardened policy is tag-independent denial: any object carrying `FILE_ATTRIBUTE_REPARSE_POINT` is denied before it contributes content or traversal authority.

Assertions:

- no bytes from a reparse target are served;
- no response reveals the target path;
- denial category is observable internally;
- direct, index, and listing paths agree;
- GET and HEAD agree on status and body suppression;
- denial does not leak handles or permits.

## Track C — Namespace and normalization matrix

Exercise request and filesystem names involving:

- drive prefixes;
- UNC forms;
- `\\?\` extended paths;
- `\\.\` device paths;
- alternate data streams;
- reserved DOS device names with case and extensions;
- trailing spaces and dots;
- repeated separators;
- forward/backslash ambiguity;
- percent-encoded separators and colons;
- encoded dot components;
- double encoding;
- long components and long total paths;
- non-ASCII, surrogate-pair, and combining names;
- case-insensitive aliases;
- 8.3 short-name aliases where enabled.

Required behavior:

- unsafe/ambiguous request forms are rejected before filesystem access;
- valid Unicode names resolve by correct UTF-16 code-unit length;
- no parser-normalized name unexpectedly opens a distinct Windows object;
- ADS and device namespace access is impossible;
- short-name alias behavior is documented and tested; if it can bypass dotfile/reserved-name policy, disable the hardened profile on volumes where it cannot be controlled or detect/reject the condition.

## Track D — Concurrent mutation race harness

Build a race harness with independent mutator and requester processes or threads.

Mutation scenarios:

- regular file ↔ reparse point;
- directory ↔ junction;
- parent directory replacement;
- index file replacement;
- direct file delete/recreate;
- file ↔ directory replacement;
- rename chains;
- permission/ACL removal and restoration;
- root pathname rename/replacement;
- listing churn;
- same-name file replacement during range streaming.

The harness must identify every allowed source object by stable identity and content digest. For every successful response, verify that the bytes came from an allowlisted object opened beneath the pinned root and not from a denied reparse target.

Safe outcomes include serving one valid opened version, 404, 403, or connection termination according to the documented mapping. Root escape, mixed content from different files, or serving a denied object is a release-blocking failure.

## Track E — Root identity and deployment replacement behavior

Test the pinned-root contract under Windows sharing and rename semantics.

Required cases:

- configured root pathname renamed after startup;
- new directory created at the old pathname;
- files atomically replaced inside the retained root where permitted;
- root delete-pending behavior;
- server restart after root replacement;
- old root handles retained during in-flight streams;
- new requests continue using the pinned root identity until restart.

Document Windows-specific restrictions caused by open directory handles. Production deployment guidance should recommend a tested content-update strategy rather than assuming Unix rename behavior.

## Track F — File identity, validators, and range consistency

Using the validator work from Plan 082, verify Windows metadata produces sufficiently strong representation identity.

Cases:

- same-size rapid replacement;
- same timestamp granularity;
- rename over existing file;
- hard links, if permitted and relevant;
- range request during replacement;
- conditional request after replacement;
- direct file versus directory index identity.

ETags must change when the served representation changes under the documented strong/weak validator contract. If Windows identity metadata cannot guarantee a strong validator, use a clearly weak validator and document its semantics.

## Track G — ACL, sharing, and error behavior

Test:

- unreadable root;
- unreadable intermediate directory;
- unreadable final file;
- sharing violation;
- delete-pending object;
- file removed after open;
- directory removed after enumeration;
- access revoked during streaming;
- handle quota exhaustion;
- memory pressure where practical.

Requirements:

- no panic;
- no path leakage;
- typed internal category;
- stable public status where a response is possible;
- listener and future requests remain healthy;
- failed opens do not consume permits permanently.

## Track H — Resource stability and shutdown

Run repeated and concurrent tests for:

- direct files;
- ranges;
- directory index lookup;
- directory listing;
- denied reparse requests;
- missing paths;
- slow clients;
- client disconnects;
- graceful shutdown;
- forced shutdown;
- repeated server start/stop.

Measure:

- process handle count;
- memory/RSS or working set;
- active tasks/threads;
- connection permits;
- file/listing permits;
- shutdown completion time;
- sockets left in active states.

Acceptance requires return to a stable baseline with no monotonic handle, task, or permit growth.

## Track I — Installed artifact parity

Run the critical Windows qualification subset against:

- workspace-built binary;
- packaged standalone binary, if published;
- installed Python wheel CLI;
- in-process Python server primitive;
- Rust external-consumer fixture.

Verify all use the same Windows resolver implementation and source revision. Capture SHA-256 hashes of tested artifacts.

Do not accept source-tree tests as evidence for a wheel or release binary.

## Track J — Fuzz and corpus replay

Add/replay Windows-focused corpora for:

- request component parsing;
- UTF-16 conversion and `UNICODE_STRING` construction;
- directory buffer parsing;
- Windows error/status mapping;
- namespace rejection;
- reparse tag handling.

Fuzz goals:

- no panic;
- no integer overflow;
- no malformed pointer/length pair;
- no path-policy bypass;
- bounded allocation;
- deterministic category mapping.

## Track K — Independent Windows safety review

Require a reviewer who did not author the implementation to inspect:

- all Windows FFI declarations and struct layouts;
- constants against current SDK headers/documentation;
- every unsafe block;
- ownership transfer and duplication;
- UTF-16 lengths;
- relative-open semantics;
- reparse detection timing;
- directory record parsing;
- path fallback reachability;
- race harness methodology.

High or critical findings block profile promotion. Medium findings must be corrected or narrow the support profile explicitly.

## Track L — Profile decision

At completion, produce a signed/recorded decision for each profile:

### `windows-reverse-proxy`

Eligible for `supported-hardened` only if:

- local NTFS only;
- loopback/private origin behind a mature proxy;
- symlink/reparse following disabled;
- directory listing disabled unless Plan 085 gates pass for the release;
- all required exact-SHA gates pass;
- no unresolved high/critical findings;
- installed artifact evidence exists.

### `windows-direct-https`

Remain `candidate` or `functional` until Plans 087–089 also qualify native TLS, internet runtime behavior, artifacts, soak, and release closure.

### `windows-functional`

Remain functional-only for SMB, non-NTFS, cloud-placeholder, or unsupported filesystem classes.

### `link-following-compat`

Remain outside the hardened guarantee.

Promotion must update `release/support-profiles.toml`, generated docs, release checklist, and release notes atomically. No profile may be promoted based only on unit tests.

## Required tests

At minimum:

- all reparse classes available in the dedicated VM;
- intermediate/final/index/listing denial;
- outside-root target never served;
- Unicode and namespace matrix;
- 8.3 alias behavior;
- concurrent mutation race harness;
- root replacement identity;
- validator replacement cases;
- ACL/sharing failures;
- resource stability;
- graceful and forced shutdown;
- installed wheel/binary parity;
- fuzz corpus replay.

## Release-gate changes

Add a grouped Windows qualification set:

- `windows.reparse-matrix`;
- `windows.namespace-matrix`;
- `windows.race-root-escape`;
- `windows.root-identity`;
- `windows.validator-identity`;
- `windows.resource-stability`;
- `windows.installed-artifact`;
- `windows.independent-safety-review`;
- `windows.profile-decision`.

The release aggregator must fail closed for:

- skipped privileged fixtures;
- stale SHA;
- source/artifact mismatch;
- unsupported filesystem;
- missing independent review;
- open high/critical finding.

## Documentation changes

Update all platform tables and:

- README;
- deployment guide;
- security policy;
- threat model;
- Windows ADR;
- filesystem architecture;
- release contract;
- release checklist;
- support profiles;
- known limitations;
- content update/deployment guidance.

State the exact hardened boundary: Windows version family, architecture, local NTFS, no reparse following, and any listing restrictions.

## Acceptance criteria

- Complete direct/index/listing paths are handle-relative.
- All available reparse classes are denied by default.
- Race testing serves zero bytes outside the pinned root.
- Unicode and namespace tests pass on production paths.
- Resource measures remain bounded.
- Installed artifacts match source behavior.
- Independent review has no unresolved high/critical finding.
- Profile status is updated only from exact-SHA evidence.
- Unsupported filesystems remain explicitly functional-only.

## Stop conditions

Do not promote Windows if:

- any hardened path still reconstructs authority from an absolute path;
- required reparse fixtures cannot be executed on any dedicated environment;
- a race can serve an outside-root object;
- handle/resource growth is monotonic;
- FFI layout or ownership remains uncertain;
- installed artifacts cannot be tied to the source SHA;
- independent review is unavailable.

## Handoff

After Release D closes, Plans 087–089 may complete the operational, performance, internet-deployment, and final release qualification required for unqualified production-grade claims.