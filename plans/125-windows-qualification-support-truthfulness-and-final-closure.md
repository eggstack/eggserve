# Plan 125 — Windows Qualification, Support Truthfulness, and Final Closure

## Status

**READY FOR HANDOFF — 2026-08-11.**

Parent roadmap: Plan 120.
Depends on: Plans 121–124.

Reviewed baseline:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
```

Relevant existing work:

```text
Plan 086 — Windows Adversarial Filesystem Qualification
Plans 084–085 — Windows handle-relative confinement work
Plans 112–119 — scope/verification/distribution consolidation
```

This plan does not replace Plan 086's threat model. It closes the gap between the existing Windows implementation/test scaffold and the support claims EggServe can honestly make today, while also performing final public-documentation/verification closure for Plan 120.

---

## Current support position

The reviewed public documents intentionally remain conservative:

- Windows functionality exists and uses handle-relative confinement;
- the README says independent adversarial qualification is incomplete;
- `SECURITY.md` says Windows should not be used with untrusted mutable public content;
- Unix is the fully hardened reference posture.

That is preferable to an unsupported claim. The remaining task is to produce stronger Windows evidence where feasible and then update wording **only to the level actually proven**.

The README also still contains internal implementation-history prose referring directly to Plans 108/109 in the normative security section. Public docs should state the invariant, not the sequence of planning records that produced it.

Platform documentation also mixes two different concepts:

```text
source/runtime support
prebuilt wheel availability
```

For example, source support lists Linux aarch64 and macOS x86_64, while the reviewed manual release workflow builds Linux x86_64, macOS arm64, and Windows x86_64 wheels. These are not the same support claim and should be documented separately.

---

## Goal

Close the remaining security/support/documentation uncertainty without adding new product scope:

1. execute the existing Windows adversarial fixtures on the strongest available Windows environment;
2. distinguish executed pass/fail evidence from fixtures that cannot be created on the available host;
3. correct implementation defects only when a reproducible confinement/correctness failure is demonstrated;
4. retain the current Windows warning if full adversarial qualification cannot be completed;
5. remove internal planning-history language from normative public documentation;
6. separate source-supported platforms from prebuilt wheel targets;
7. perform one final verification pass and close Plans 120–125 without creating another polish roadmap.

---

## Non-goals

Do not:

- redesign Windows confinement preemptively;
- add sandboxing, ACL management, impersonation, Windows service integration, containers, seccomp-equivalents, or WAF behavior;
- support SMB/ReFS/FAT/cloud-placeholder content as hardened deployment targets;
- enable reparse/symlink following by default;
- add Windows to every routine CI check merely for qualification;
- create permanent dedicated infrastructure if one-shot/manual evidence is sufficient;
- claim hardened Windows parity when security-relevant fixtures remain unexecuted;
- turn historical plan documents into user documentation by deleting/rewording their records;
- expand the release wheel matrix simply to make the support table symmetric.

---

## Track A — Reconcile Plan 086 with the current implementation

Before running new tests, audit Plan 086 against current source/tests and create a compact evidence matrix with categories:

```text
requirement / fixture
current automated test location
ordinary hosted Windows runnable? yes/no
privilege/environment requirement
last known result if recorded
current status: pass / fail / blocked-fixture / not implemented
```

At minimum cover the security-significant Plan 086 groups:

```text
file/directory symlink denial
junction/reparse denial
intermediate/final reparse components
root reparse behavior
index/listing reparse entries
ADS/device/UNC/extended namespace rejection
reserved DOS names
trailing dot/space ambiguity
encoded/backslash/colon ambiguity
Unicode/UTF-16 handling
root replacement/pinned identity
file/directory replacement races
range/validator replacement behavior
ACL/sharing failures
handle/resource cleanup
shutdown during active streams
installed-wheel behavior
```

Do not duplicate an existing test solely to make the matrix look complete.

### Acceptance criteria

- every Plan 086 security category is mapped to current evidence or explicitly marked missing/blocked;
- ordinary “test passed” is not used when fixture creation silently skipped;
- no claim relies only on test count (e.g. “114 tests”) without identifying what security classes those tests exercise;
- the matrix is a closure record, not a new permanent verification registry.

---

## Track B — Run hosted Windows evidence without growing routine CI

Use the existing/manual Windows build environment or a temporary/manual workflow execution to run all tests that can execute meaningfully on a standard GitHub-hosted Windows runner.

This should include:

```text
Rust Windows unit/integration tests
Windows filesystem/path hardening tests that can create their fixtures
Python wheel build/install tests
stock static-serving smoke
compatibility facade smoke
TLS smoke where already supported
Plan 121 lifecycle regressions
Plan 122 installed CLI/python -m/subprocess checks
```

If a small targeted workflow adjustment is necessary to expose a currently unrun Windows test suite during the manual release workflow, it may be added to the **manual release/qualification** path. Do not add a third routine merge CI job unless there is a demonstrated frequently-regressing portability defect that cannot be covered elsewhere.

### Acceptance criteria

- exact Windows runner version/architecture/source SHA is recorded;
- every relevant test reports pass/fail/blocked-fixture explicitly;
- installed wheel is tested from an isolated environment;
- no source-tree/PATH artifact masks wheel behavior;
- routine CI remains the current small shape.

---

## Track C — Execute privileged/reparse/race evidence where feasible

Plan 086 correctly identified that some adversarial fixtures may require capabilities not available on ordinary hosted runners.

If a suitable disposable Windows VM/environment is available, run the remaining high-value fixtures there, prioritizing:

1. junction/reparse traversal denial;
2. root/intermediate/final object replacement races;
3. pinned-root replacement behavior;
4. symlink/reparse index/listing cases;
5. namespace/ADS/device-path ambiguity;
6. handle leakage under repeated denied/racing requests.

Record:

```text
Windows edition/build
filesystem/volume
privilege level
Developer Mode/symlink privilege
source SHA
fixture creation result
request result
whether bytes from any denied/outside-root object were observable
```

If no suitable environment is available, **do not manufacture one through large CI engineering in this plan**. Mark those classes `blocked-fixture`/`not independently qualified` and retain the public Windows restriction.

### Security failure definition

Any reproducible case where safe-default serving returns bytes from an object outside the pinned root or from a denied reparse target is release-blocking.

Other safe outcomes may include 403, 404, or connection failure according to the established error mapping.

### Acceptance criteria

- adversarial results identify the exact fixture and observed object identity/content;
- blocked privilege/environment conditions are recorded as blocked, not skipped/pass;
- no outside-root or denied-reparse bytes are served in any passing qualification case;
- handle/file-stream permits recover after denied/disconnected/racing cases;
- test harness changes remain Windows-focused and do not alter product runtime unless a defect is reproduced.

---

## Track D — Correct only demonstrated Windows defects

If Tracks B/C expose a real implementation defect, fix the smallest layer that owns the violated invariant.

Priority order for reasoning:

```text
request-target/path rejection
SecureRoot / handle-relative traversal
reparse attribute checks
opened-handle identity/lifetime
response planning/streaming
error mapping
```

Do not add secondary security wrappers around a broken primitive instead of fixing the primitive.

Every security fix requires a regression test using the same fixture class.

If no defect is reproduced, make no Windows runtime changes merely to “harden further”.

### Acceptance criteria

- every runtime change is tied to a reproducible failing fixture;
- regression fails before and passes after the fix;
- Unix behavior remains unchanged unless shared code was actually defective;
- no new dependency or platform abstraction framework is introduced without necessity.

---

## Track E — Make the Windows support claim evidence-exact

After qualification, choose one of two explicit outcomes.

### Outcome 1 — adversarial qualification complete enough for promotion

Only if all security-significant Plan 086 fixture classes are executed successfully on a suitable Windows environment may docs promote the platform beyond the current warning.

Even then, state the precise support boundary: local NTFS-like filesystem, safe-default no-reparse-follow behavior, tested Windows versions/architectures, and exclusions such as SMB/ReFS/cloud placeholders if unqualified.

### Outcome 2 — qualification remains incomplete

If meaningful reparse/race/root-identity fixture classes remain blocked, retain wording equivalent to:

> Windows is functional and uses handle-relative confinement, but independent adversarial qualification is incomplete; do not use it for untrusted mutable public content.

Improve specificity by naming the unqualified evidence classes rather than implying the implementation itself is known broken.

This outcome is an acceptable closure. Lack of an appropriate Windows adversarial environment is not justification for CI/infrastructure over-engineering.

### Acceptance criteria

- support wording follows executed evidence rather than implementation confidence;
- no “hardened” claim is made solely because normal Windows tests pass;
- incomplete qualification can close this plan while retaining the warning;
- docs clearly distinguish implementation capability from adversarial assurance.

---

## Track F — Remove planning-history leakage from normative docs

Public/normative documentation should describe current invariants rather than planning provenance.

At minimum inspect:

```text
README.md
SECURITY.md
docs/security-policy.md
docs/python-api.md
docs/python-http-server-compatibility.md
docs/python-packaging.md
docs/toolchain-support.md
architecture/* only where presented as current architecture
```

The reviewed README contains:

```text
Plan 108 is retained as a historical corrective implementation...
Verified Plan 109 completed...
```

Replace that user-facing paragraph with the actual invariant, for example that every running server owns one shared file-stream admission pool used by static/Rust/Python file responses.

Do not rewrite or sanitize historical plan files themselves.

Search for other current-doc references to plan numbers that are necessary only to explain implementation history. Keep a plan reference only when it is genuinely useful as optional historical evidence, not as the normative definition of behavior.

### Acceptance criteria

- README security/API claims stand alone without needing plan numbers;
- public docs state implementation invariants directly;
- historical `plans/` files remain untouched records;
- no new documentation hierarchy or duplicate architecture document is created.

---

## Track G — Separate source support from prebuilt artifact support

Audit current release/build reality.

The reviewed manual release workflow produces:

```text
Linux x86_64 wheel
macOS arm64 wheel
Windows x86_64 wheel
```

while the README source/platform table also lists:

```text
Linux aarch64
macOS x86_64
```

Document these as separate dimensions.

Recommended structure:

```text
Runtime/source-supported platforms
    Linux x86_64 / aarch64
    macOS arm64 / x86_64
    Windows x86_64 (with qualification wording)

Prebuilt Python wheels produced by the current release workflow
    Linux x86_64
    macOS arm64
    Windows x86_64
```

Do not expand the wheel matrix merely to eliminate the distinction. If release artifacts have changed by implementation time, document the actual current matrix instead of copying this baseline.

Also ensure installation text does not imply a prebuilt wheel on a source-supported architecture where users would actually build from source.

### Acceptance criteria

- source support and prebuilt distribution support are separately stated;
- listed wheel targets match the actual release workflow;
- no new release job is added only for documentation symmetry;
- CPython 3.11+ abi3 claim remains accurate.

---

## Track H — Final post-Plan-120 dependency/artifact sanity check

After Plans 122–124, inspect the final release artifacts once more.

Use existing tooling (`cargo tree`, binary/wheel member sizes, and `cargo bloat` only if already available/easy to install locally) to identify whether a newly dominant dependency/artifact is an obvious removable duplication.

This is **not** another dependency slimming phase.

Only make a dependency change here if all are true:

1. it is directly unused/redundant after Plans 122–124;
2. removal requires a small diff;
3. no feature/API/security behavior changes;
4. full verification proves the removal.

Otherwise record “no further justified reduction” and stop.

### Acceptance criteria

- no speculative dependency rewrite is started;
- no pure-Rust replacement project is opened for a small dependency without evidence;
- final artifact composition is recorded once and closed.

---

## Track I — Final verification and closure record

Run the normal verification posture, not an expanded one:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Also run the targeted evidence from Plans 121–124:

```text
readiness timeout/state regressions
installed `eggserve` command and `python -m eggserve`
ServerProcess child-process smoke
wheel composition/size evidence
stock SimpleHTTPRequestHandler fast-path/fallback tests if implemented
runtime multi-server lifecycle/resource evidence if changed
Windows qualification matrix from this plan
```

Append a concise closure record to Plan 125 containing:

```text
implementation commit(s)
verification commands/results
wheel before/after size summary
performance/resource decision summaries
Windows qualification outcome
remaining explicit platform limitations
```

Do not create Plan 126 for minor prose/formatting observations after these criteria pass.

---

## Explicit acceptance criteria

Plan 125 and the parent Plan 120 roadmap are complete only when:

- [ ] Plan 086 requirements are mapped to current executable evidence;
- [ ] hosted Windows tests are run and fixture-blocked cases are distinguished from passes;
- [ ] privileged/reparse/race tests are executed where a suitable environment exists, or explicitly recorded as unqualified if none exists;
- [ ] no outside-root/denied-reparse content is served in any passing adversarial test;
- [ ] any demonstrated Windows defect has a focused regression test and correction;
- [ ] Windows support wording is no stronger than the executed evidence;
- [ ] the current warning remains if independent adversarial qualification is incomplete;
- [ ] README normative sections no longer rely on Plan 108/109 history to define runtime invariants;
- [ ] source-supported platforms and prebuilt wheel targets are separately documented and match reality;
- [ ] no release matrix expansion is added solely for symmetry;
- [ ] final artifact/dependency check finds no obvious duplicate left by the closure track, or any small justified cleanup is completed;
- [ ] normal Rust/TLS/Python installed-wheel verification passes;
- [ ] Plans 121–124 have closure evidence for their acceptance criteria;
- [ ] routine CI remains small and release publication remains manual;
- [ ] Plan 120 is marked complete without creating a new polish roadmap.

---

## Rejection conditions

Reject the implementation if it:

- treats uncreatable Windows fixtures as passes;
- promotes Windows hardened/public-untrusted support based only on ordinary CI;
- adds a large permanent Windows CI/VM apparatus instead of retaining an honest qualification warning;
- implements speculative Windows security layers without a failing fixture;
- rewrites historical plans to make current documentation look cleaner;
- claims wheel availability for architectures not produced by the release process;
- expands release automation or product scope;
- creates another numbered follow-up solely for documentation wording after the closure criteria pass.
