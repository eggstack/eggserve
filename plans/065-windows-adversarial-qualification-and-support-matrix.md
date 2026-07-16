# Phase 65 — Windows Adversarial Qualification and Support Matrix

## Goal

Qualify the Windows handle-relative implementation under real reparse, namespace, race, packaging, and long-run resource tests. Promote only the configurations supported by evidence, and leave all other Windows environments explicitly functional-only or unsupported.

This phase is evidence-driven. Code changes should be corrective and test-enabling rather than feature expansion.

## Preconditions

- Plan 063 implements pinned-root, component-relative, deny-all-reparse resolution.
- Plan 064 serves files, indexes, ranges, and listings from validated handles.
- Candidate gates pass in ordinary Windows CI.
- Windows remains unpromoted in public support metadata.

## Non-goals

Do not:

- broaden hardened support to every Windows filesystem;
- add reparse-tag allowlists;
- support SMB or cloud-placeholder roots without dedicated evidence;
- add write operations;
- weaken name rejection for compatibility;
- add application-server or edge-server features;
- equate generic CI success with adversarial filesystem qualification.

## Qualification target

The initial hardened Windows profile should be deliberately narrow:

- supported Windows releases declared by the project;
- x86_64 unless additional targets have installed-artifact evidence;
- local NTFS volume;
- ordinary non-reparse root directory;
- safe defaults;
- no symlink/reparse following;
- operator-controlled read-only or administratively controlled content;
- reverse-proxy origin and, after Plan 069, optional direct TLS.

Everything else remains functional-only or unsupported until separately qualified.

## Track A — Dedicated Windows test environment

Create a reproducible Windows qualification environment capable of:

- enabling Developer Mode or required symlink privileges;
- creating file and directory symlinks;
- creating junctions;
- creating mount-point reparse objects where feasible;
- creating or importing test fixtures with unknown/custom reparse tags where safe;
- creating alternate data streams;
- enabling/disabling 8.3 names where required for tests;
- running standalone binaries and installed wheels;
- measuring process handle count and memory;
- collecting full logs and evidence artifacts.

Use a dedicated VM or self-hosted runner for tests unavailable on generic GitHub-hosted runners. Document setup and teardown. Tests must be non-destructive outside their temporary volume/tree.

## Track B — Reparse-point matrix

Test final and intermediate components for:

- file symbolic links;
- directory symbolic links;
- junctions;
- mount points;
- volume mount points where safe;
- unknown/custom tags;
- cloud placeholder tags if available;
- other Microsoft-defined tags present on the qualification host.

For every case:

- direct GET;
- HEAD;
- range request;
- index lookup;
- directory listing;
- concurrent access;
- replacement during resolution.

Expected hardened behavior:

- no traversal;
- no target content read;
- deterministic policy rejection;
- sanitized response/log behavior;
- connection remains protocol-correct;
- handle count returns to baseline.

Adopt deny-all semantics rather than relying on tag-specific knowledge.

## Track C — Namespace and normalization matrix

Test request-target and filesystem fixtures for:

- drive-qualified forms;
- UNC forms;
- device namespaces;
- extended-length prefixes;
- `GLOBALROOT`-style namespaces;
- reserved DOS names with and without extensions;
- case variants of reserved names;
- alternate data streams;
- trailing dot and trailing space aliases;
- repeated separators and encoded separators;
- NUL and malformed percent encodings;
- short-name/8.3 aliases;
- case-only filename collisions;
- Unicode normalization and confusable cases within the documented policy;
- names near component and total target limits.

The test should distinguish parser rejection from filesystem rejection and prove no unsafe normalization occurs.

## Track D — Race and replacement harness

Build a stress harness with one or more mutator threads/processes and concurrent HTTP clients.

Mutation cases:

- regular file ↔ file symlink;
- directory ↔ directory symlink;
- directory ↔ junction;
- file A ↔ file B atomic replacement;
- index file ↔ reparse point;
- parent directory replacement;
- root pathname rename/replacement;
- directory listing entry replacement;
- truncation/growth during full and range streaming;
- permission changes;
- deletion and recreation.

For every served body, record:

- expected allowlisted content digest;
- opened file identity where test instrumentation permits;
- root identity;
- response status and headers;
- whether mutation was active.

Fail immediately if:

- content comes from outside the allowlisted root dataset;
- a reparse target is served;
- a hidden replacement request is processed after a framing error;
- handle count grows without returning to baseline;
- the process panics or deadlocks.

## Track E — Filesystem support matrix

Probe and classify at least:

- local NTFS;
- ReFS if infrastructure is available;
- FAT/exFAT removable or virtual volume if available;
- SMB/network share;
- cloud-synced placeholder directory;
- WSL interop paths if relevant;
- third-party filesystem only when readily available.

For each, record:

- root open behavior;
- file identity support;
- relative-open behavior;
- reparse semantics;
- rename/replacement behavior;
- directory enumeration behavior;
- installed artifact tests;
- resulting classification.

Do not expand hardened support merely because smoke tests pass. Only local NTFS is expected to be promoted in this phase unless equivalent evidence is deliberately produced.

## Track F — Resource and lifecycle qualification

Run Windows-specific stress for:

- many successful requests;
- many 404/403/reparse rejections;
- slow clients;
- cancelled full and range streams;
- listing cancellation;
- graceful shutdown under load;
- forced shutdown;
- repeated server start/stop through Python;
- callback exceptions if Python server primitives are in the tested profile;
- TLS when built with the feature, as a precursor to Plan 069.

Track:

- process handle count;
- working set/private bytes;
- thread count;
- Tokio task/permit instrumentation where available;
- socket states;
- file-stream permits;
- shutdown duration.

Use stable trend thresholds rather than exact platform-noisy counts.

## Track G — Installed artifact qualification

Test both:

- standalone Windows binary;
- installed CPython wheel containing the native extension and bundled binary.

Tests must run outside the source tree and verify:

- CLI static serving;
- `python -m eggserve`;
- native `SecureRoot`;
- in-process `Server` static behavior;
- range and conditional responses;
- reparse denial;
- root replacement behavior;
- shutdown and cleanup;
- package metadata/support wording.

Artifact SHA and source commit must be recorded.

## Track H — Release gates and promotion logic

Add required Windows hardened gates for:

- pinned root identity;
- component-relative traversal;
- reparse matrix;
- namespace matrix;
- race harness;
- handle-based file/index/listing operations;
- handle/resource stability;
- installed binary;
- installed wheel;
- dedicated-runner evidence freshness.

Promotion logic must fail closed if:

- any required evidence is missing or stale;
- evidence source SHA differs from release SHA;
- only generic CI ran privileged tests as skipped;
- filesystem type is not the supported type;
- a support document claims more than the metadata permits.

## Track I — Documentation and operator guidance

Publish:

- exact Windows versions and architectures supported;
- local NTFS requirement;
- unsupported filesystem/root classes;
- deny-all reparse policy;
- behavior of `--follow-symlinks` or equivalent weaker mode;
- recommended service account and ACL posture;
- recommendation to make content read-only to the service account where possible;
- reverse-proxy deployment guidance;
- limitations of direct TLS pending Plan 069;
- root replacement/restart semantics.

Do not claim protection from privileged local attackers, kernel compromise, malicious filesystem drivers, or hostile administrators.

## Corrective pass policy

Any failure must be categorized:

- implementation defect;
- test harness defect;
- unsupported platform/filesystem behavior;
- documentation/claim defect;
- environmental inability to gather evidence.

Implementation defects require correction and full rerun. Unsupported environments require explicit classification, not silent skips. Missing external evidence blocks promotion.

## Acceptance criteria

Windows local NTFS may be promoted to `supported-hardened` only if:

- every tested final/intermediate reparse object is denied;
- namespace/normalization corpus produces no bypass;
- sustained replacement races produce zero root escape;
- file/index/listing paths use validated handles;
- handle and memory behavior remain bounded;
- installed binary and wheel pass outside the source tree;
- dedicated Windows evidence is current and tied to the final SHA;
- documentation and release metadata name the narrow supported profile.

## Stop conditions

Do not promote Windows if:

- privileged reparse tests could not run;
- race evidence is absent;
- unsupported filesystem fallback is indistinguishable from hardened behavior;
- installed artifact evidence is missing;
- any response serves bytes through a denied reparse point;
- resource leakage remains unresolved.

## Handoff

Plans 066–070 may proceed independently for Unix/internet runtime work. Plan 071 must combine the promoted Windows profile with cross-platform race and fault-injection qualification before final production closure.
