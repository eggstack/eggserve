# Plan 110 — Documentation Closure Polish

## Status

**COMPLETE — 2026-08-06.**

Documentation-only polish pass. No runtime code, tests, manifests, workflows,
or scripts were modified. Final reproduction/lifecycle/closure metadata polish
completed by Plan 111.

This is a documentation-only polish pass against repository state:

```text
3b75bd621a90a94fc5d732a1afb4f36e03b255dd
```

Plan 109 is functionally complete. Its runtime ownership, static-service construction,
request-body wire behavior, release-profile alignment, and hosted CI evidence do not
need another implementation pass.

A post-closure review found a small number of documentation defects:

1. the filesystem-confinement documentation conflates the one-time pinned-root
   lifecycle with the per-request `RootGuard` lifecycle;
2. the Plan 109 closure record calls the verified implementation commit the final
   `main` head even though a later documentation-only closure commit moved `main`;
3. Plan 109 retains a stale handoff note that describes already-completed work as
   remaining;
4. the binary-size record contains the final measurements and hashes but does not
   preserve the full clean, capture, and comparison procedure needed to prevent
   target-path overwrite or stale-artifact ambiguity;
5. nearby architecture prose should be checked for wording that implies
   `StaticService` owns transport admission rather than producing file-backed
   responses that are admitted by the server-owned `RuntimeState`.

No runtime defect is reopened by this plan. The required work is precise terminology,
closure-record cleanup, and reproducibility documentation only.

---

## Goal

Make the active documentation describe the verified implementation exactly:

```text
static server construction
    -> ServeState pins one filesystem root
    -> StaticService retains that static state

per request
    -> StaticService::call / plan_static_request
    -> RootGuard::new against the pinned root
    -> clone the pinned descriptor or handle for request-scoped traversal
    -> resolve and plan the response
    -> drop the request-scoped guard and its clone

server startup
    -> one RuntimeState per running Server
    -> one file-stream semaphore in RuntimeState

response transport
    -> StaticService or custom Service returns a canonical response
    -> runtime transport conversion acquires file admission for file-backed bodies
    -> body completion, cancellation, error, or drop releases admission
```

The documentation must also distinguish the three relevant Plan 109 commits:

```text
cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d
    functional implementation candidate

d273134aa7eb1583106afc00f4e24dc09e0aeb91
    artifact measurements and evidence correction

49ecb712be1677a027891ad373b6951d7b916182
    final verified implementation tree tested by hosted CI

3b75bd621a90a94fc5d732a1afb4f36e03b255dd
    documentation-only Plan 109 closure commit
```

Use exact terms consistently:

- **pinned root**: long-lived filesystem authority retained by static state;
- **RootGuard**: request-scoped traversal guard created from the pinned root;
- **RuntimeState**: server-scoped transport state, including file-stream admission;
- **verified implementation tree**: the exact source/test tree exercised by hosted CI;
- **closure commit**: a later documentation-only commit recording that evidence;
- **artifact candidate**: the exact functional SHA from which recorded release artifacts
  were measured.

---

## Scope

This plan authorizes documentation edits only.

Primary files:

```text
architecture/filesystem-confinement.md
architecture/eggserve-core.md
benchmarks/binary-size.md
plans/109-final-admission-and-wire-verification-corrective-pass.md
plans/110-documentation-closure-polish.md
```

Conditionally inspect and edit only when an actual contradictory statement is found:

```text
architecture/runtime.md
architecture/overview.md
docs/architecture.md
docs/release-process.md
docs/python-packaging.md
README.md
AGENTS.md
.opencode/skills/eggserve-dev/SKILL.md
release/plan-102-106-closure.md
plans/102-runtime-correctness-scope-and-size-roadmap.md
plans/107-runtime-streaming-and-closure-corrective-pass.md
plans/108-static-metadata-and-runtime-closure-follow-up.md
```

Do not churn these conditional files merely to repeat the same wording.

---

## Non-goals

Do not:

- change Rust or Python source code;
- change tests, fixtures, benchmarks, or conformance corpora;
- change Cargo manifests, lockfiles, profiles, or feature sets;
- change CI or release workflows;
- change wheel-building or package-verification scripts;
- rerun or replace the recorded Plan 109 artifact measurements without evidence of an
  actual measurement error;
- create a new benchmark framework;
- reopen Plan 109 runtime ownership or wire-semantics work;
- remove the deprecated compatibility adapter;
- redesign `ServeState`, `StaticService`, `RuntimeState`, or `RootGuard`;
- add HTTP features or broaden project scope;
- add a permanent documentation linter or CI job;
- rewrite historical plans as though their original repository state never existed;
- claim that a documentation-only commit received functional verification unless that
  exact commit was actually tested.

The final implementation diff for Plan 110 must contain documentation files only.

---

## Required implementation order

Implement in this order:

```text
A. Establish the documentation baseline and claim inventory
B. Correct pinned-root and RootGuard lifecycle terminology
C. Correct runtime-admission ownership wording
D. Repair the Plan 109 closure record and remove stale handoff language
E. Complete artifact-reproduction documentation
F. Reconcile nearby active documentation
G. Verify a documentation-only diff and close Plan 110
```

Do not start by performing broad prose cleanup. First identify the exact false or
ambiguous claims, then make the smallest coherent edits.

---

# Track A — Establish the baseline and claim inventory

## A1 — Record the immutable evidence anchors

Before editing, confirm and preserve these facts:

| Purpose | Commit |
|---|---|
| Plan 109 functional implementation | `cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d` |
| Artifact-evidence documentation | `d273134aa7eb1583106afc00f4e24dc09e0aeb91` |
| Final hosted-CI-tested implementation tree | `49ecb712be1677a027891ad373b6951d7b916182` |
| Plan 109 documentation closure | `3b75bd621a90a94fc5d732a1afb4f36e03b255dd` |
| Hosted CI run | `31035414453` |

Do not collapse these into one generic “candidate SHA.” Each serves a different
purpose.

## A2 — Search active documentation for affected claims

Search for at least the following terms and phrases:

```sh
rg -n "RootGuard|PinnedRoot|pinned root|created once|per request|per-request" \
  architecture docs README.md AGENTS.md .opencode release plans

rg -n "final main head|final `main` head|implementation tree|functional candidate|closure commit" \
  architecture docs README.md AGENTS.md .opencode release plans benchmarks

rg -n "StaticService.*semaphore|StaticService.*admission|file-stream semaphore|file admission" \
  architecture docs README.md AGENTS.md .opencode release plans

rg -n "remaining work|handoff note|delete the alternate admission|structurally unavoidable" \
  plans/109-final-admission-and-wire-verification-corrective-pass.md

rg -n "Candidate SHA|SHA-256|target/release/eggserve|target/dist/eggserve|staged" \
  benchmarks docs release plans
```

Classify each result as:

- correct current-state documentation;
- correct historical description;
- ambiguous ownership wording;
- factually incorrect current-state wording;
- stale implementation instruction after closure.

Historical descriptions should remain historical. Do not rewrite prior-state analysis
unless its present-tense wording incorrectly claims that the old state is current.

## A3 — Keep a narrow edit inventory

The implementation handoff should list every file selected for editing and the exact
claim being corrected. The expected minimum inventory is:

```text
architecture/filesystem-confinement.md
    distinguish one-time PinnedRoot construction from per-request RootGuard creation

architecture/eggserve-core.md
    clarify that runtime transport, not StaticService, owns file admission

plans/109-final-admission-and-wire-verification-corrective-pass.md
    distinguish verified implementation tree from later closure commit
    remove or archive the stale remaining-work handoff note

benchmarks/binary-size.md
    preserve the clean unique-artifact capture and hash-comparison procedure
```

Add other files only when the search finds a concrete inconsistency.

## Acceptance criteria

- the four immutable evidence anchors are recorded correctly;
- affected claims have been inventoried before editing;
- historical statements are not silently rewritten into a false chronology;
- no unrelated prose-cleanup scope is introduced.

---

# Track B — Correct filesystem lifecycle terminology

## B1 — State the actual object lifetimes

The documentation must distinguish these objects:

### `PinnedRoot`

- constructed during `ServeState::new` when static state is built;
- validates and pins the configured root;
- retained for the lifetime of the static service state;
- provides the filesystem authority from which request traversal begins;
- is not reconstructed or re-canonicalized for every request.

### `RootGuard`

- constructed inside static request planning for each request;
- borrows or references the already-pinned root identity;
- clones the root descriptor on Unix or root handle on Windows for that request;
- carries request-scoped traversal authority;
- is dropped after request planning, closing the request-scoped clone;
- does not repin the configured pathname.

### Resolved file or directory handles

- are produced during request-scoped traversal;
- retain the authority needed for response planning or streaming;
- must not be described as pathname reopens under the hardened profile;
- have lifetimes separate from the long-lived pinned root and the request planning guard.

## B2 — Replace the inaccurate lifecycle sequence

In `architecture/filesystem-confinement.md`, replace wording equivalent to:

```text
RootGuard is created once when StaticService is built
```

with a sequence equivalent to:

```text
1. ServeState pins the configured root once during static-service construction.
2. Each static request creates a RootGuard from that pinned root.
3. RootGuard clones the pinned root descriptor or handle for request-scoped traversal.
4. Resolution uses that request-scoped authority without reopening the configured root
   pathname.
5. The request-scoped guard is dropped after planning; any file handle retained by the
   canonical response follows its own streaming lifetime.
```

Use the repository’s actual platform distinctions:

- Unix hardened mode: descriptor-relative traversal;
- Windows hardened mode: handle-relative traversal;
- explicitly documented fallback/follow-symlink modes retain their existing weaker
  qualification.

Do not imply that one `RootGuard` is shared concurrently across requests.

## B3 — Audit related diagrams and tables

Check nearby lifecycle diagrams, filesystem-authority tables, and path-resolution steps
for the same conflation. Correct only the entries that are wrong.

Preferred wording:

```text
one pinned root per static service
one request-scoped RootGuard per static request
```

Avoid vague wording such as:

```text
one root guard per server
static service owns the request guard
root is opened on every request
```

## Acceptance criteria

- no active document says `RootGuard` is constructed once at static-service build time;
- active documentation says the root itself is pinned once;
- active documentation says `RootGuard` is request-scoped;
- descriptor/handle cloning and drop behavior are described without implying pathname
  revalidation;
- no platform hardening guarantee is broadened beyond the existing implementation.

---

# Track C — Correct runtime-admission ownership wording

## C1 — Preserve the ownership boundary

The verified ownership boundary is:

```text
ServeState
    owns static configuration and pinned filesystem state

StaticService
    owns or shares ServeState
    plans canonical static responses
    owns no transport semaphore
    acquires no file-stream permit

RuntimeState
    is created once per running Server
    owns the file-stream semaphore
    is shared across connections

transport conversion
    converts canonical responses to Hyper responses
    acquires runtime admission for file-backed bodies
```

Every active architecture statement must agree with this boundary.

## C2 — Correct misleading `StaticService` feature summaries

In `architecture/eggserve-core.md`, wording such as:

```text
StaticService: File-stream semaphore-gated concurrency
```

is too easy to read as static-service ownership. Replace it with wording equivalent to:

```text
StaticService produces canonical file-backed responses; the server runtime applies
shared file-stream admission during transport conversion.
```

Concise table form is acceptable:

```text
File-backed responses admitted by the server-owned RuntimeState at transport conversion
```

Do not remove the fact that static responses are admission-controlled. Correct the
owner and boundary.

## C3 — Check compatibility-adapter wording

The deprecated `service` adapter may be described as requiring an explicit
caller-supplied `RuntimeState`. It must not be described as:

- owning an independent pool;
- constructing runtime state per call;
- being the production server path;
- bypassing admission.

Retain its deprecated and migration-only status.

## Acceptance criteria

- active documentation attributes the semaphore to `RuntimeState` only;
- `StaticService` is described as a canonical response planner/service, not an
  admission owner;
- file-backed static, custom Rust, and Python responses are described as sharing
  server runtime admission in production;
- the compatibility adapter remains clearly deprecated and explicit-context only;
- no new public architecture or API is implied.

---

# Track D — Repair the Plan 109 closure record

## D1 — Correct commit-role terminology

In the Plan 109 closure record, replace:

```text
The final main head is 49ecb...
```

with language equivalent to:

```text
The final verified implementation tree is
49ecb712be1677a027891ad373b6951d7b916182. Hosted CI run 31035414453 checked out
and tested that exact tree. The later Plan 109 closure commit
3b75bd621a90a94fc5d732a1afb4f36e03b255dd changed documentation only.
```

The record should make these distinctions explicit:

- `cea39...` contains the functional implementation;
- `d273...` records measured artifact evidence;
- `49ec...` is the exact implementation/evidence tree exercised by hosted CI;
- `3b75...` closes the plan in documentation after CI.

Do not call `3b75...` the functional candidate. Do not imply that `49ec...` remained
`main` HEAD after the closure commit.

## D2 — Remove the stale remaining-work handoff note

The end of Plan 109 currently retains language equivalent to:

```text
The runtime implementation is close. The remaining work is...
```

That statement contradicts the verified-complete status.

Preferred correction:

- remove the stale handoff section entirely; or
- replace it with a short archival note explaining that the section was superseded by
  the verified closure record.

Recommended archival replacement:

```text
## Archival note

The implementation handoff that originally followed this section is superseded by the
verified closure above. No runtime work remains under Plan 109. Subsequent documentation
terminology and reproducibility polish is tracked by Plan 110.
```

Do not leave imperative implementation steps beneath a `COMPLETE` status unless clearly
marked as historical and superseded.

## D3 — Preserve truthful CI scope

Retain the statement that hosted CI tested `49ec...` and that the only post-functional
change before that run was the rustfmt correction.

Do not claim:

- that the documentation-only closure commit itself received the same hosted run;
- that artifact measurements were regenerated by CI;
- that Plan 109 added new CI jobs;
- that scheduler performance comparison was performed.

## D4 — Preserve historical plan chronology

Plans 102, 107, and 108 may continue to state that Plan 109 reclosed their bounded
runtime-ownership claims. Do not rewrite their original issue descriptions.

Only correct active summary wording if it misidentifies the tested tree or current plan
state.

## Acceptance criteria

- `49ec...` is called the final verified implementation tree, not the permanent final
  `main` head;
- `3b75...` is identified as the documentation-only closure commit;
- the hosted CI run is tied to the exact tested SHA;
- no stale remaining-work instructions remain active at the end of Plan 109;
- Plan 109 remains marked complete;
- no runtime acceptance criterion is reopened or weakened.

---

# Track E — Complete artifact-reproduction documentation

## E1 — Preserve existing measured values

Unless a concrete inconsistency is discovered, retain the recorded Plan 109 values:

```text
Candidate SHA:
cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d

Default release CLI: 1,966,408 bytes
Default dist CLI: 856,920 bytes
TLS release CLI: 3,075,040 bytes
TLS dist CLI: 1,218,048 bytes
Bundled default dist CLI: 856,920 bytes
Native extension: 2,255,752 bytes
Wheel: 1,573,717 bytes

Default dist CLI SHA-256:
f7b69951e629796672073bc110f7f968d8479d482b3a578bac2f69a1eeb669b9

TLS dist CLI SHA-256:
9aa1a5ece3b2ae3bce9aaaf59822e3c88e9fffbcf2fe37d7b8fd2a8e1c4033e4

Linux CPython 3.14 wheel SHA-256:
8502e5e8f4961920a40f1d13955d7cfc75a7bac797033ec169da0c222ac40d40
```

This plan does not authorize inventing missing hashes or replacing measurements with a
new machine’s output merely for prose polish.

## E2 — Document clean-state preparation

Add a reproducibility subsection that starts from a clean artifact state. It should
include commands equivalent to:

```sh
rm -rf target/release target/dist
rm -rf crates/eggserve-python/target
rm -rf crates/eggserve-python/python/eggserve/bin
rm -rf dist
```

Do not recommend `cargo clean` as the only option if the intent is to preserve unrelated
build caches. The documented commands should specifically remove paths capable of
contaminating these measurements.

## E3 — Capture each CLI artifact immediately

The default and TLS builds use the same target filenames. The procedure must not build
all variants and measure the shared path afterward.

Document immediate capture into unique paths, for example:

```sh
artifact_dir="$(mktemp -d)"

cargo build --release --locked -p eggserve-bin
cp target/release/eggserve "$artifact_dir/eggserve-default-release"

cargo build --profile dist --locked -p eggserve-bin
cp target/dist/eggserve "$artifact_dir/eggserve-default-dist"

cargo build --release --locked -p eggserve-bin --features tls
cp target/release/eggserve "$artifact_dir/eggserve-tls-release"

cargo build --profile dist --locked -p eggserve-bin --features tls
cp target/dist/eggserve "$artifact_dir/eggserve-tls-dist"
```

Use a platform-neutral note for `cp`, `stat`, and `sha256sum` equivalents where
necessary. The recorded snapshot itself is Linux `x86_64-unknown-linux-gnu`.

## E4 — Measure unique captured artifacts

Document size checks against the unique paths, not the mutable Cargo target paths:

```sh
stat --printf='%n %s\n' "$artifact_dir"/eggserve-*
sha256sum "$artifact_dir"/eggserve-*
```

The prose must explain:

- `target/release/eggserve` is overwritten by the later TLS release build;
- `target/dist/eggserve` is overwritten by the later TLS dist build;
- unique copies are required to preserve artifact identity;
- profile and stripping differences must not be presented as code-size-only changes.

## E5 — Verify packaged CLI identity

After the supported wheel script builds and stages the default non-TLS `dist` CLI,
document extraction and comparison of:

```text
unique default dist capture
staged Python-package CLI
wheel-extracted CLI member
```

The procedure must require SHA-256 equality among those three files.

Use placeholders when paths depend on the wheel filename, but make the relationship
explicit. A representative sequence may be documented as:

```sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh

sha256sum \
  "$artifact_dir/eggserve-default-dist" \
  crates/eggserve-python/python/eggserve/bin/eggserve \
  "$artifact_dir/eggserve-wheel-extracted"
```

The implementation may use Python’s `zipfile` module to extract the wheel member if that
is the clearest portable example. Do not add a permanent script solely for this plan.

## E6 — Preserve scheduler-evidence honesty

Retain the existing statement that:

- the standalone CLI uses Tokio current-thread runtime;
- the recorded 1 KiB workload is a bounded suitability smoke measurement;
- no current-thread versus multithread performance comparison was performed;
- lifecycle coverage is functional evidence, not throughput evidence;
- large-file, range, TLS, and cancellation suites are correctness coverage rather than
  permanent performance gates.

Do not expand this documentation polish into new measurements.

## Acceptance criteria

- the record retains the full artifact candidate SHA;
- existing measured sizes and hashes remain unchanged unless a proven transcription
  error is documented;
- clean-state preparation is explicit;
- each default/TLS and release/dist CLI is copied immediately to a unique path;
- size and hash commands operate on unique captured files;
- the shared-target-path overwrite hazard is explicit;
- staged and wheel-extracted bundled CLI identity is verified against default dist by
  hash;
- native-extension and wheel measurement semantics remain clear;
- no unperformed benchmark is claimed;
- no new script, CI job, or release gate is added.

---

# Track F — Reconcile nearby active documentation

## F1 — Check architecture summaries

Review active summaries for agreement with these statements:

```text
one pinned root per static service
one request-scoped RootGuard per static request
one RuntimeState per running server
one runtime-owned file-stream admission pool
StaticService owns no transport semaphore
49ec... is the hosted-CI-tested implementation tree
3b75... is the documentation-only Plan 109 closure commit
```

Edit only statements that contradict these facts.

## F2 — Check release and packaging summaries

Ensure active release documentation continues to state:

- standalone distribution CLI uses `dist`;
- wheel-bundled CLI is the default non-TLS `dist` binary;
- the native extension uses its explicitly equivalent distribution profile;
- GitHub Actions verifies/builds but does not publish;
- publication remains manual;
- artifact identity must be established by exact path and hash;
- TLS and default artifacts must not be confused because Cargo target paths are reused.

Do not reopen the release-process simplification track.

## F3 — Check plan-state summaries

Plan summaries should say:

- Plans 102–109 are historical implementation records;
- Plan 109 is verified complete;
- Plan 110 is documentation polish only;
- Plan 110 does not supersede Plan 109’s runtime closure authority;
- no functional corrective plan is active for this track.

Do not add Plan 110 references throughout the repository unless needed to prevent a
false active-state claim.

## F4 — Avoid duplicated doctrine

Prefer one authoritative detailed explanation and short cross-references elsewhere.
Do not copy the entire lifecycle or artifact procedure into multiple architecture files.

Recommended authority locations:

```text
architecture/filesystem-confinement.md
    filesystem object lifecycle

architecture/eggserve-core.md and architecture/runtime.md
    concise service/runtime ownership boundary

benchmarks/binary-size.md
    artifact measurement and reproduction procedure

plans/109-final-admission-and-wire-verification-corrective-pass.md
    Plan 109 closure evidence and commit-role chronology
```

## Acceptance criteria

- active architecture documents agree on object ownership and lifetimes;
- active release documents agree on artifact profile and identity;
- plan-state summaries distinguish implementation closure from documentation polish;
- no unnecessary file churn or duplicated long-form doctrine is introduced.

---

# Track G — Verification and truthful closure

## G1 — Enforce a documentation-only diff

Before closure, inspect the complete diff:

```sh
git diff --name-only <base-sha>...HEAD
git diff --stat <base-sha>...HEAD
git diff <base-sha>...HEAD
```

Allowed changed paths are documentation files only. Reject the implementation if the
diff includes:

```text
*.rs
*.py
Cargo.toml
Cargo.lock
pyproject.toml
.github/workflows/**
scripts/**
conformance/**
fuzz/**
```

An incidental source-formatting change is not acceptable in this plan.

## G2 — Run structural text checks

Run targeted checks equivalent to:

```sh
! rg -n "RootGuard.*created once|created once.*RootGuard" architecture docs README.md

! rg -n "The final `main` head is `49ecb|final main head.*49ecb" \
  architecture docs README.md AGENTS.md .opencode release plans benchmarks

! rg -n "The runtime implementation is close|delete the alternate admission ownership|make one runtime state structurally unavoidable" \
  plans/109-final-admission-and-wire-verification-corrective-pass.md

rg -n "request-scoped RootGuard|per-request RootGuard" architecture/filesystem-confinement.md
rg -n "verified implementation tree" plans/109-final-admission-and-wire-verification-corrective-pass.md
rg -n "documentation-only.*closure commit|closure commit.*documentation-only" \
  plans/109-final-admission-and-wire-verification-corrective-pass.md
rg -n "overwrit|unique.*artifact|SHA-256" benchmarks/binary-size.md
```

Adjust exact patterns to the final prose, but preserve the intent.

## G3 — Run basic repository hygiene

Required:

```sh
git diff --check
```

Run the repository’s existing markdown-link or documentation check only if one already
exists and is lightweight. Do not add a new linter.

Because this is a documentation-only plan, a full runtime test matrix is not required
for implementation confidence. Existing CI may still run normally after push; do not
present that as necessary proof of unchanged runtime semantics when the diff already
contains no executable files.

## G4 — Review claims against code

Before marking complete, manually verify:

- `ServeState` retains a pinned root and no semaphore;
- `plan_static_request` constructs `RootGuard` per request;
- `RuntimeState` owns the file-stream semaphore;
- server startup constructs one runtime state and shares it across connections;
- Plan 109’s hosted run checked out `49ec...`;
- current `main` includes the later documentation closure commit.

This is a claim-validation review, not a request to modify the code.

## G5 — Closure record

Append a concise closure record to Plan 110 containing:

- implementation commit full SHA;
- base SHA;
- files edited;
- exact documentation checks run;
- confirmation that no source, tests, manifests, workflows, or scripts changed;
- confirmation that Plan 109 remains functionally closed;
- confirmation that measured artifact values were preserved or an explicit explanation
  of any proven correction;
- hosted CI result if it ran, without overstating its necessity or scope.

Do not alter Plan 109’s functional candidate or hosted run identifiers during Plan 110
closure.

---

## Expected implementation diff

The preferred minimal implementation modifies four existing documents and closes this
plan:

```text
architecture/filesystem-confinement.md
architecture/eggserve-core.md
benchmarks/binary-size.md
plans/109-final-admission-and-wire-verification-corrective-pass.md
plans/110-documentation-closure-polish.md
```

Additional active documentation may be edited only when Track A identifies a concrete
contradiction.

Expected change characteristics:

- mostly replacement of inaccurate lifecycle and ownership phrases;
- one clarified commit chronology in Plan 109;
- deletion or archival replacement of the stale Plan 109 handoff tail;
- one reproducibility subsection in the binary-size record;
- no source or configuration changes;
- no dependency or artifact changes.

---

## Recommended commit sequence

A single implementation commit is appropriate because this is a narrow documentation
pass:

```text
docs: correct Plan 109 closure terminology and artifact reproduction
```

A separate closure commit is optional. Use one only when hosted checks need to be
recorded after the implementation commit. Do not create multiple mechanical prose
commits.

---

## Rejection conditions

Reject the implementation as incomplete if any of the following is true:

- an active document still says `RootGuard` is constructed once per static service;
- active documentation fails to distinguish `PinnedRoot` from `RootGuard`;
- `StaticService` is still described as owning the file-stream semaphore;
- Plan 109 still calls `49ec...` the final `main` head without qualification;
- the later `3b75...` closure commit is omitted from the chronology;
- stale remaining-work instructions remain active after the verified closure record;
- artifact reproduction measures mutable Cargo target paths only after all variants are
  built;
- the target-path overwrite hazard remains undocumented;
- packaged CLI identity is asserted from size alone rather than hash equality;
- existing recorded artifact values are changed without a documented evidentiary basis;
- an unperformed scheduler comparison is implied;
- runtime, test, build, workflow, or script files are modified;
- a new CI or publication mechanism is introduced;
- Plan 109 is reopened as functionally incomplete;
- broad prose churn obscures the narrow corrections.

---

## Final acceptance checklist

### Filesystem lifecycle

- [x] one pinned root per static service is documented;
- [x] one request-scoped `RootGuard` per static request is documented;
- [x] descriptor/handle clone and drop behavior is accurate;
- [x] no pathname reopen is implied under hardened traversal;
- [x] platform qualification remains unchanged.

### Runtime admission ownership

- [x] `RuntimeState` is the sole documented semaphore owner;
- [x] `StaticService` is documented as producing canonical responses;
- [x] transport conversion is documented as applying file admission;
- [x] compatibility-adapter wording remains deprecated and explicit-context only.

### Plan 109 closure record

- [x] `cea39...` is identified as the functional implementation candidate;
- [x] `d273...` is identified as the artifact-evidence commit;
- [x] `49ec...` is identified as the exact hosted-CI-tested implementation tree;
- [x] `3b75...` is identified as the documentation-only closure commit;
- [x] CI run `31035414453` remains tied to `49ec...`;
- [x] stale active handoff instructions are removed or archived;
- [x] Plan 109 remains functionally complete.

### Artifact reproduction

- [x] clean-state artifact removal is documented;
- [x] default release, default dist, TLS release, and TLS dist are captured uniquely;
- [x] shared target-path overwrite risk is explicit;
- [x] sizes and hashes are taken from unique captures;
- [x] staged and wheel-extracted bundled CLI identity is checked by SHA-256;
- [x] existing measured values remain stable absent proven error;
- [x] scheduler claims remain truthful;
- [x] no permanent benchmark or release gate is added.

### Verification and scope

- [x] changed paths are documentation-only;
- [x] `git diff --check` passes;
- [x] targeted stale-phrase checks pass;
- [x] no source, tests, manifests, workflows, or scripts change;
- [x] no scope expansion occurs;
- [x] Plan 110 receives a concise closure record after implementation.

---

## Handoff summary

This is a bounded documentation correction, not another runtime corrective pass.

The implementer should:

1. inventory the exact affected claims;
2. distinguish the long-lived pinned root from the request-scoped `RootGuard`;
3. attribute file-stream admission exclusively to server-owned `RuntimeState`;
4. distinguish Plan 109’s functional, evidence, verified-tree, and closure commits;
5. remove the stale Plan 109 remaining-work tail;
6. preserve a complete unique-artifact capture and hash-comparison procedure;
7. verify that the diff is documentation-only;
8. close Plan 110 without reopening Plan 109 or expanding CI.

No runtime work remains authorized under this plan.

---

## Closure record — 2026-08-06

**Implementation commit:**
`dc66811130ebdb43eb605bcaba823a2854287549`

**Files edited:** 9 documentation/planning files. Eight pre-existing repository
documents were corrected, and Plan 110 itself was updated with status and
closure evidence:

- `architecture/filesystem-confinement.md` — distinguished one-time `PinnedRoot` construction from per-request `RootGuard` creation
- `architecture/eggserve-core.md` — corrected `StaticService` feature summary to attribute file-stream admission to runtime transport
- `architecture/overview.md` — updated plan-state summaries
- `benchmarks/binary-size.md` — added clean-state preparation, unique-artifact capture, and SHA-256 verification procedure
- `docs/architecture.md` — corrected `RootGuard` description to borrow-from-pinned-root wording
- `plans/109-final-admission-and-wire-verification-corrective-pass.md` — distinguished verified implementation tree from closure commit, removed stale handoff
- `AGENTS.md` — updated plan-state summaries
- `.opencode/skills/eggserve-dev/SKILL.md` — updated plan-state summaries
- `plans/110-documentation-closure-polish.md` — updated with status and closure evidence (self-update)

**Verification run:**

- `git diff --name-only` — 9 documentation/planning files
- `git diff --check` — clean
- `cargo fmt --all -- --check` — pass
- `cargo clippy --workspace --lib --bins --tests -- -D warnings` — pass
- `cargo test --workspace` — 1,366 passed, 9 ignored
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings` — pass
- `cargo test -p eggserve-bin --features tls` — 88 passed

**Stale-phrase checks (post-edit):**

- `RootGuard.*created once` — no matches in active docs
- `The final main head is 49ecb` — no matches in active docs (only in Plan 110 itself describing what to fix)
- `The runtime implementation is close` — no matches in Plan 109
- `request-scoped` present in `filesystem-confinement.md` — confirmed
- `verified implementation tree` present in Plan 109 — confirmed
- `documentation-only.*closure commit` present in Plan 109 — confirmed

**Confirmations:**

- No source, tests, manifests, workflows, or scripts changed
- Plan 109 remains functionally closed
- Existing measured artifact values in `benchmarks/binary-size.md` were preserved
- No new CI job, release gate, or publication mechanism was added
- No scheduler comparison was claimed

**Post-closure review note:**

Post-closure review identified three documentation-only residuals: the artifact
reproduction commands depended on temporary files removed by the wheel test
harness, Windows `RootGuard` wording overstated root-handle duplication, and
closure bookkeeping needed reconciliation. Plan 111 corrects those documentation
issues without reopening Plan 109 or changing runtime behavior.