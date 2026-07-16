# Phase 71 — Cross-Platform Filesystem Race and Fault Injection

## Goal

Prove that the completed Unix and Windows confinement implementations remain safe under sustained concurrent mutation and operational faults. Build a common evidence harness that records the identity and digest of every served object and fails on any response outside the allowlisted pinned-root dataset.

## Preconditions

- Plan 061 pins root identity on Unix.
- Plans 063–065 complete the qualified Windows path where Windows production is claimed.
- Plan 070 qualifies the live HTTP connection state machine.

## Non-goals

Do not add:

- protection against kernel compromise or privileged attackers;
- write APIs;
- filesystem snapshots or version control;
- content synchronization;
- hot root replacement;
- generalized chaos infrastructure;
- performance optimizations that weaken open-handle identity.

## Security invariant

> Under all tested filesystem mutations, every successful response body must come from an ordinary object opened beneath the pinned root under the active hardened policy. Failure is acceptable; serving an unapproved object is not.

## Track A — Common mutation-harness model

Create a platform-neutral controller with platform-specific mutators.

The harness should define:

- pinned server root;
- allowlisted ordinary files and known content digests;
- forbidden external tree with unique sentinel digests;
- mutator actions;
- concurrent client workers;
- request IDs;
- opened file identity instrumentation where available;
- response status, headers, body digest, and timing;
- process/resource metrics;
- deterministic seed and run duration.

Any forbidden sentinel body is an immediate high-severity failure.

## Track B — Unix mutation suite

On Linux and macOS, repeatedly perform:

- regular file ↔ symlink replacement;
- directory ↔ symlink replacement;
- parent component replacement;
- final component replacement;
- atomic file rename/exchange where supported;
- index file replacement;
- directory listing entry replacement;
- root pathname rename/replacement;
- deletion/recreation;
- permission removal/restoration;
- truncation and growth during streaming;
- hard-link substitution where relevant;
- bind mount or mount-boundary probes in a privileged test environment where feasible.

Validate descriptor-relative `O_NOFOLLOW` behavior and same-open-object streaming.

## Track C — Windows mutation suite

Reuse and extend Plan 065 cases:

- file ↔ symbolic link;
- directory ↔ symbolic link;
- directory ↔ junction;
- mount-point reparse substitution;
- unknown reparse tag substitution;
- index and listing entry replacement;
- root pathname rename/replacement;
- file A ↔ file B atomic replacement;
- deletion/recreation;
- permission changes;
- truncation/growth;
- case/short-name alias probes.

Require local NTFS for hardened evidence. Other filesystems remain classification probes only.

## Track D — Response identity accounting

Test builds should expose internal, non-public evidence sufficient to associate a response with:

- pinned root identity;
- opened file identity;
- metadata snapshot;
- response representation length;
- content digest computed by the harness;
- path fixture category.

Do not expose raw local paths or handles in production logs/API.

For large files, use deterministic content blocks so partial/range responses can be validated without hashing an unrelated full file on every request.

## Track E — Streaming mutation semantics

Define and test expected behavior when an already opened ordinary file changes:

- pathname replacement should not change the opened object;
- in-place writes/truncation may affect reads according to OS semantics and are not a confinement escape;
- response framing must remain safe if fewer bytes are readable than planned;
- the server must not append bytes from a replacement object;
- range responses must not read outside planned bounds;
- errors after response start close safely;
- no retry by pathname is permitted.

Recommend immutable/atomic content deployment or read-only content for strongest representation consistency.

## Track F — Filesystem error injection

Inject or simulate:

- root access failure at startup;
- component access denied;
- metadata failure;
- file read failure after headers;
- seek failure;
- directory enumeration failure;
- deleted index candidate;
- too many open files/handles;
- transient sharing violation on Windows;
- invalid/unsupported object type;
- filesystem unavailable/unmounted where safely testable.

Verify typed internal errors, sanitized client behavior, deterministic connection disposition, and cleanup.

## Track G — Runtime fault injection

Combine filesystem load with:

- listener errors where injectable;
- connection saturation;
- file-stream saturation;
- slow clients;
- write timeout;
- graceful shutdown;
- forced shutdown;
- TLS handshake load;
- malformed HTTP corpus replay;
- Python callback exception/stall for the callback-capable profile;
- log sink failure or broken pipe where safe.

Avoid test-only fault hooks in stable APIs. Use internal feature-gated hooks or dependency injection at narrow boundaries.

## Track H — Root replacement semantics

Prove on every hardened platform:

1. Start with root A and record identity.
2. Rename root A.
3. Create root B at the original pathname with forbidden sentinel content.
4. Request existing and new names.
5. Verify running server never serves root B.
6. Restart/reconstruct server.
7. Verify new instance may intentionally pin root B.

Document behavior for entries created within the still-open root A after rename according to OS semantics.

## Track I — Long race runs

Define:

- PR smoke duration;
- scheduled nightly/weekly duration;
- pre-release minimum operation count and wall time;
- number of mutators and clients;
- required platforms;
- privileged runner requirements;
- evidence artifact format.

Record:

- seed;
- commit SHA;
- OS/kernel/filesystem;
- binary/wheel identity;
- operation counts;
- response counts/statuses;
- forbidden digest count;
- errors/panics;
- resource trends.

## Required tests

- deterministic one-shot race regressions;
- concurrent stress on Linux and macOS;
- concurrent stress on qualified Windows NTFS;
- root replacement suite;
- index/listing mutation suite;
- range/full-file mutation suite;
- descriptor/handle exhaustion recovery;
- shutdown during mutation;
- installed artifact subset;
- no forbidden digest assertion.

Use process isolation for tests that change resource limits or mount state.

## Corrective workflow

For every anomaly:

- preserve seed and operation log;
- preserve served digest and identity evidence;
- classify confinement escape, representation race, expected OS behavior, resource leak, or harness defect;
- minimize to deterministic regression where possible;
- correct code or narrow support profile;
- rerun all affected platform evidence.

Any confirmed root escape blocks all production profiles on that platform.

## Release criteria

Add non-waivable security gates for:

- Linux race qualification;
- macOS race qualification;
- Windows NTFS race qualification when claimed;
- root replacement invariant;
- no forbidden digest;
- fault-injection cleanup;
- evidence freshness and environment metadata.

Invalidate on filesystem, response streaming, range planning, index/listing, root configuration, platform FFI, and packaging changes.

## Documentation

Publish:

- mutation threat assumptions;
- root replacement behavior;
- in-place file mutation limitations;
- recommended atomic/read-only deployment patterns;
- supported filesystem matrix;
- fault behavior and sanitized errors;
- evidence summary without exposing sensitive runner paths.

## Acceptance criteria

- Sustained mutation produces zero root escape and zero forbidden sentinel bytes.
- Root pathname replacement cannot retarget a running server.
- Final opened-object identity is preserved through streaming.
- Faults do not leak descriptors/handles, tasks, permits, or sockets.
- Response framing remains safe under truncation/read failure.
- Evidence exists on every production-claimed platform/filesystem.

## Stop conditions

Do not proceed to final qualification if:

- any root escape is unresolved;
- identity evidence cannot distinguish allowed from forbidden objects;
- Windows privileged mutation tests are skipped;
- filesystem errors trigger pathname retry/reopen;
- resource exhaustion leaves the server permanently degraded;
- support claims exceed tested filesystems.

## Handoff

Plan 072 carries these race artifacts into long-duration soak, installed-artifact, observability, and provenance qualification.
