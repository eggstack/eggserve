# Plan 075 — Corrective Baseline and Release Containment

## Goal

Establish a reproducible corrective baseline for eggserve, classify the identified defects against support profiles, and prevent release or support-profile promotion while critical corrective gates are incomplete.

This plan does not fix the defects themselves. It creates the evidence, release gating, and documentation framework required for Plans 076–083 to make trustworthy closure claims.

## Preconditions

- Plan 074 is present and defines the corrective release sequence.
- The current default branch includes the implemented work through Plan 073 and subsequent Windows fixes.
- Existing CI, support-profile metadata, release criteria, and evidence aggregation remain available.

## Non-goals

Do not:

- change runtime, filesystem, or HTTP behavior except where required to expose deterministic tests;
- promote any support profile;
- add product features;
- broadly refactor CI unrelated to evidence quality;
- waive failing tests to obtain a green baseline;
- replace dedicated platform evidence with parser-only or compile-only evidence.

## Track A — Pin the corrective baseline

Create a corrective status document recording:

- exact baseline commit SHA;
- Rust toolchain and lockfile identity;
- Python versions and wheel-build toolchain;
- operating-system and architecture matrix;
- enabled feature combinations;
- existing support-profile classifications;
- latest known successful evidence per gate;
- all known failing, skipped, or absent gates;
- known environmental blockers.

The baseline document must distinguish source-level review findings from executable reproductions. Do not present a suspected issue as closed merely because a nearby test passes.

## Track B — Finding registry

Create a machine-readable or strictly structured finding registry for the corrective program.

Each finding must record:

- stable identifier;
- title;
- severity;
- affected subsystem;
- affected support profiles;
- first-observed commit;
- reproduction status;
- owning plan;
- required regression evidence;
- documentation impact;
- closure commit and reviewer when complete.

At minimum register the findings listed in Plan 074.

Severity guidance:

- Critical/high: possible memory/handle unsafety, confinement failure, handler invocation despite rejection, false release evidence, detached work after claimed shutdown where externally exploitable, or material protocol ambiguity.
- Medium: incorrect timeout semantics, API values silently ignored, inaccurate metadata, HEAD/validator inconsistencies, operational hot loops, or bounded leaks.
- Low/informational: performance opportunities and documentation cleanup without correctness impact.

When severity is uncertain, classify conservatively until reproduction narrows it.

## Track C — Reproduction inventory

For each Release A–C finding, identify the smallest deterministic reproduction and where it belongs.

Required categories:

- Windows UTF-16 component names through production resolver paths;
- Windows handle ownership and duplication failure;
- long-lived keep-alive connection exceeding the nominal write timeout;
- active large response progressing beyond the timeout;
- stalled response writer;
- shutdown with more active tasks than can finish before deadline;
- custom-service builder retaining or discarding its service;
- real local/remote address propagation;
- body rejection before handler side effects;
- incomplete-body close/drain behavior;
- frontend configuration parity;
- zero and out-of-range direct Rust configuration;
- direct file versus directory index conditionals and ranges;
- HEAD success, error, listing, range, and 304 behavior;
- same-size rapid file replacement and validator change.

Do not force every reproduction into unit tests. Use raw TCP, integration tests, installed wheel tests, or dedicated Windows runners where those are the actual production boundaries.

## Track D — CI and release-gate mapping

Map each finding to the workflows and release criteria invalidated by changes in its subsystem.

Required mappings:

- Windows FFI or resolver changes invalidate dedicated Windows Unicode, handle, reparse, streaming, installed artifact, and safety-review gates.
- Runtime timeout/task changes invalidate connection lifecycle, keep-alive, shutdown, soak, TLS feature, and Python server gates.
- body-policy changes invalidate raw framing, service invocation, drain/close, keep-alive reuse, and Python callback gates.
- configuration changes invalidate Rust API, CLI, Python constructor, snapshot, migration, and installed artifact gates.
- response planner changes invalidate canonical conformance, raw-wire, production-path, range, conditional, HEAD, and proxy-origin gates.

Update the release aggregator so missing, stale, skipped, or wrong-SHA evidence fails closed.

## Track E — Support-profile containment

Review `release/support-profiles.toml` and all generated/public support tables.

Required temporary classifications:

- Windows hardened profiles remain candidate or functional-only until Release D closes.
- Any profile relying on corrected shutdown or body rejection semantics may not be promoted while Release A or B is open.
- Public documentation must not imply that JSON logging, active drain, real connection metadata, or write-idle semantics exist before their implementing plans close.

Add a corrective-program marker that release tooling can use to block production-profile promotion while blocking findings remain open.

## Track F — Baseline test execution

Run the existing complete matrix on the pinned SHA:

- formatting;
- Clippy with warnings denied;
- workspace tests and doctests;
- feature combinations;
- canonical HTTP conformance;
- raw-wire tests;
- production-path tests;
- Unix filesystem race/confinement tests;
- Windows compile and dedicated runtime tests;
- Python unit tests;
- wheel build and installed-wheel tests;
- packaging and binary smoke tests;
- dependency/security/license checks;
- benchmark smoke checks where already required.

Capture failures without modifying expectations to hide them.

## Track G — Evidence storage and naming

Define a stable evidence location and naming convention for Plans 075–083.

Every evidence record must include:

- plan number;
- test/gate name;
- source SHA;
- workflow run or command;
- platform/architecture;
- feature set;
- artifact hash where applicable;
- pass/fail/blocked result;
- timestamp;
- failure excerpt or artifact reference.

Evidence generated against one SHA must not be silently reused after an invalidating code change.

## Track H — Corrective dashboard/status file

Create a concise status file for handoff agents and reviewers showing:

- Releases A–C and constituent plans;
- plan state: not started, active, blocked, implemented, evidence pending, independently reviewed, closed;
- blocking findings;
- latest implementation SHA;
- latest complete evidence SHA;
- next unblocked plan;
- known environmental requirements.

The status file is descriptive. It must not replace the finding registry or release aggregator.

## Required tests

Add tests for release tooling itself:

- open high-severity finding blocks release;
- missing required gate blocks release;
- stale SHA blocks release;
- skipped dedicated Windows gate blocks Windows promotion;
- documentation/support metadata disagreement blocks release;
- closed finding without regression evidence blocks release;
- low-severity deferred finding with valid disposition does not block unrelated narrow profiles.

## Documentation changes

Update:

- corrective roadmap index;
- release criteria;
- support-profile documentation;
- contributor/reviewer guidance for corrective evidence;
- known limitations;
- migration/release notes placeholder for Releases A–C.

The README should contain only a brief status statement and link to authoritative support metadata rather than duplicating the full finding registry.

## Acceptance criteria

- The exact corrective baseline SHA and toolchain are recorded.
- Every Plan 074 finding has a stable registry entry and owning plan.
- Each Release A–C finding has a deterministic reproduction strategy.
- Release tooling fails closed for open blocking findings, stale evidence, skipped required jobs, and documentation/profile mismatch.
- Windows hardened support cannot be promoted before Release D.
- The complete existing matrix has a captured baseline result.
- Evidence naming and invalidation rules are documented and tested.
- A status file identifies the next unblocked corrective plan without overstating closure.

## Stop conditions

Stop and document rather than weakening gates if:

- a required dedicated platform is unavailable;
- current CI cannot distinguish skipped from passed work;
- support claims cannot be derived consistently from machine-readable metadata;
- baseline failures expose a new critical/high issue;
- artifact/source identity cannot be established.

## Handoff

After this plan closes, Plans 076 and 077 are unblocked and may proceed in parallel. All later plans must update the finding registry, evidence records, and corrective status file as part of closure.