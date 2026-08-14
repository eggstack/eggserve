# Plan 127 — Final Plan 126 Release and Callback Closure

## Status

**READY FOR HANDOFF — 2026-08-14.**

Reviewed state:

```text
main                           = 455fb5c076f2d940cc0ab5982bee6e29eba18f4c
plan-126-corrective-closure    = a32026bf3e98e4ed11ce09d5012d2c17a5b31aba
PR                             = #2
```

This is not a new roadmap. It is a deliberately tiny closure plan for the residual work found after reviewing the Plan 126 implementation on PR #2.

Plan 126 remains the governing corrective plan. Plan 127 exists only because a post-implementation audit found two concrete unclosed acceptance items and the user explicitly requested a follow-up handoff plan.

Do not reopen fast-path architecture, packaging architecture, Windows hardening, CI design, dependency policy, benchmark infrastructure, or product scope.

---

## Current verified state

The Plan 126 branch already contains the substantive corrective work:

- exact native fast-path eligibility for stock `SimpleHTTPRequestHandler` and supported `functools.partial` shape;
- native `max_connections` mapping that restores compatibility concurrency semantics on the stock fast path;
- production-boundary HTTP and TLS admission tests;
- Plan 123 before/after benchmark evidence showing material fast-path benefit;
- release workflow conversion to the extension-backed wheel architecture with no staged standalone executable;
- wheel composition assertions;
- installed `eggserve` and `python -m eggserve` smoke commands;
- stale bundled-binary Python documentation corrections;
- append-only closure corrections for Plans 122, 123, and 125;
- green routine PR CI on the current branch head.

Do not rewrite or reimplement those areas unless needed to fix a directly observed regression.

The remaining work is only:

1. remove/fix one broken Linux-only release smoke assertion;
2. add one behavioral callback-concurrency regression test;
3. dispatch the corrected manual Release workflow and require all three existing platform jobs to pass;
4. append the final evidence to Plan 126, mark it complete, and merge PR #2.

---

## Track A — Remove the broken Linux release smoke assertion

### Problem

The Linux Release job currently contains an unnecessary assertion equivalent to:

```python
import sys
sys.exit(sys.modules["eggserve"].__name__ == "eggserve" and 0 or 1)
```

The command does not import `eggserve` first. In a fresh interpreter, `sys.modules["eggserve"]` is therefore not guaranteed to exist and normally raises `KeyError`.

The immediately following `scripts/release_smoke.py` invocation already imports and exercises the installed package by serving a real fixture. The extra assertion adds no useful coverage.

### Required correction

Prefer deletion of the redundant assertion rather than repairing it.

The Linux smoke sequence should remain conceptually:

```text
install wheel into fresh venv
installed eggserve --help
python -m eggserve --help
scripts/release_smoke.py using that venv Python
```

Do not add another import-only smoke command unless there is a demonstrated gap.

### Acceptance criteria

- [ ] the `sys.modules["eggserve"]` assertion is removed;
- [ ] Linux release smoke still checks the installed console script;
- [ ] Linux release smoke still checks `python -m eggserve`;
- [ ] Linux release smoke still serves a real fixture using `scripts/release_smoke.py`;
- [ ] no source-tree or Cargo-target executable is used by the smoke test;
- [ ] no new dependency is added.

---

## Track B — Prove callback-path concurrency behavior, not only fallback selection

### Problem

Plan 126 correctly proves that subclasses/custom handlers are ineligible for the native static fast path, but the current callback regression tests only assert selection state such as:

```text
server._native_fast_path == False
```

That proves dispatch selection, not the existing runtime contract that callback-backed `ThreadingHTTPServer(max_workers=N)` remains bounded by the Python callback semaphore.

The Plan 126 correction changed native-fast-path admission. A final behavioral test should prove that this did not accidentally alter callback concurrency semantics.

### Required test

Add one focused production-boundary test using a custom `BaseHTTPRequestHandler` or `SimpleHTTPRequestHandler` subclass that intentionally blocks inside Python request handling.

Recommended shape:

```text
ThreadingHTTPServer(max_workers=2)
custom Python callback handler

request A enters handler and blocks
request B enters handler and blocks
request C connects/sends request
prove request C does not enter handler while A+B are held
release one blocked handler
prove request C can then enter
release all requests
shutdown cleanly
```

Use deterministic synchronization primitives (`threading.Event`, `Barrier`, counters guarded by a lock) rather than latency thresholds.

The test should observe handler entry count or active-handler count directly at the Python callback boundary. Do not infer the result solely from connection closure because callback-backed servers may admit transport connections before the callback semaphore is available.

A single HTTP test is sufficient unless implementation review finds TLS uses a materially different callback-admission path. Do not duplicate the entire test for TLS without a concrete reason.

### Acceptance criteria

- [ ] a live callback-backed server is used;
- [ ] the handler is confirmed ineligible for the native fast path;
- [ ] with `max_workers=2`, at most two Python handlers are simultaneously active;
- [ ] a third request proceeds after one permit is released;
- [ ] the test is deterministic and does not depend primarily on sleep-duration assertions;
- [ ] existing callback implementation is not redesigned;
- [ ] no new semaphore/concurrency abstraction is introduced.

---

## Track C — Run the corrected manual Release workflow

### Requirement

After Tracks A and B are committed and routine PR CI is green, manually dispatch the existing `Release` workflow from the final PR #2 head.

The workflow must remain manual-only:

```yaml
on:
  workflow_dispatch:
```

Do not add push/tag publication triggers.

### Required jobs

The existing three release jobs must all complete successfully:

```text
Linux x86_64
macOS arm64
Windows x86_64
```

Each job must prove the post-Plan-122 single-artifact Python packaging model:

```text
wheel builds successfully
wheel contains no eggserve/bin/eggserve[.exe]
installed eggserve console command --help succeeds
python -m eggserve --help succeeds
real fixture serving succeeds from installed wheel
wheel artifact uploads successfully
```

### Windows interpretation

A successful Windows release job proves only ordinary build/install/runtime compatibility for that wheel.

It must **not** be recorded as adversarial Windows filesystem qualification. The existing Windows security warning remains unchanged unless separate adversarial evidence exists.

### Failure handling

If any of the three jobs fails:

1. inspect the failing step;
2. fix only the concrete release-path defect required by the existing contract;
3. rerun the Release workflow from the corrected head;
4. require all three jobs to pass in the same final implementation state.

Do not respond to a platform-specific release failure by adding a new release framework, matrix, helper dependency, container build system, or publication pipeline.

### Acceptance criteria

- [ ] Release workflow is dispatched from the final Plan 126/127 branch head;
- [ ] Linux x86_64 job passes;
- [ ] macOS arm64 job passes;
- [ ] Windows x86_64 job passes;
- [ ] wheel composition assertion passes on all three jobs;
- [ ] installed console script smoke passes on all three jobs;
- [ ] `python -m eggserve` smoke passes on all three jobs;
- [ ] real fixture serving passes on all three jobs;
- [ ] release remains `workflow_dispatch` only;
- [ ] no automated publication is introduced;
- [ ] Windows security posture is not strengthened based only on this run.

---

## Track D — Final Plan 126 closure record

Once Tracks A–C pass, update `plans/126-post-closure-fast-path-and-release-corrective-pass.md` in place.

### Required status update

Change the status from handoff/in-progress language to a completed closure state with the actual completion date.

Do not rewrite the original plan body. Append a concise closure record.

### Required evidence to record

Include:

```text
final implementation commit SHA
routine PR CI run URL/ID and conclusion
Release workflow run URL/ID and conclusion
Linux release job conclusion
macOS release job conclusion
Windows release job conclusion
callback concurrency test name/result
confirmation that Linux sys.modules smoke assertion was removed
confirmation that no standalone binary is bundled in any wheel
confirmation that Windows adversarial qualification remains incomplete
```

Mark the Plan 126 acceptance boxes complete only when supported by evidence.

If the connector/tooling cannot edit individual checkboxes safely, leave the historical body intact and append an authoritative closure table mapping every remaining criterion to evidence.

### Plan 127 closure

Append a minimal completion line to this file as part of the same closure commit. Do not create Plan 128.

### Acceptance criteria

- [ ] Plan 126 status truthfully reflects completion;
- [ ] final PR CI evidence is recorded;
- [ ] final three-platform Release evidence is recorded;
- [ ] callback concurrency behavioral evidence is recorded;
- [ ] Windows release smoke is explicitly distinguished from Windows adversarial qualification;
- [ ] Plan 127 is marked complete;
- [ ] no broad documentation sweep is performed.

---

## Track E — Merge PR #2 and stop

PR #2 is the existing handoff/implementation PR for this corrective track. Do not open another implementation PR unless repository policy or branch state makes that unavoidable.

### Pre-merge gate

Merge only when all are true:

```text
PR head contains Tracks A-D
PR is mergeable
routine CI is green on final head
manual Release workflow is green on final head
Plan 126 closure record is complete and truthful
Plan 127 closure line is present
no unexpected unrelated files changed
```

### Post-merge check

After merge, confirm:

```text
main contains the final PR head/merge result
PR #2 is merged/closed
normal main CI, if triggered, is green
```

Do not create a new plan for cosmetic wording after this point.

### Acceptance criteria

- [ ] PR #2 is merged into `main`;
- [ ] `main` contains Plan 126 implementation and Plan 127 closure;
- [ ] no unresolved release-blocking issue remains;
- [ ] no Plan 128 is created for cleanup/polish;
- [ ] the Plan 120–127 corrective track is considered closed.

---

## Verification commands

Run the normal project verification posture after the code/test edit and before the release dispatch:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Run the focused Python compatibility suite containing the callback-concurrency test directly as well.

Do not add benchmark CI. Plan 123 performance evidence is already sufficient and should not be rerun unless the final tiny code changes unexpectedly touch the fast path itself.

---

## Explicit non-goals

This final pass must not:

- change the native fast-path design again;
- change the exact partial eligibility contract;
- add a new admission semaphore;
- change Tokio runtime sizing;
- add HTTP/2, HTTP/3, ASGI, WSGI, proxying, caching, or application serving;
- add release publication automation;
- add new CI platforms or routine CI matrices;
- add dependencies;
- reopen Windows confinement design;
- perform another repository-wide documentation cleanup;
- rerun or expand benchmark infrastructure without a regression reason;
- create another roadmap.

---

## Rejection conditions

Reject an implementation that:

- leaves the broken Linux `sys.modules["eggserve"]` assertion in the Release workflow;
- treats a fallback-selection assertion as sufficient proof of callback concurrency;
- uses timing-only sleeps as the primary callback concurrency proof;
- adds another semaphore or worker scheduler to make the test pass;
- skips one of the existing three Release platform jobs;
- records a failed or stale Release run as closure evidence;
- merges PR #2 before the final Release workflow is green;
- calls Windows release smoke "hardened" or "qualified";
- adds automated release publication;
- starts Plan 128 for cosmetic leftovers.

---

## Recommended execution order

```text
1. Delete the redundant/broken Linux sys.modules release smoke assertion.
2. Add one deterministic callback-path max_workers behavioral test.
3. Run focused Python tests.
4. Run normal Rust/TLS/installed-wheel verification.
5. Push the final implementation commit to PR #2.
6. Confirm routine PR CI is green on that exact head.
7. Manually dispatch Release on that exact head.
8. Require Linux, macOS, and Windows jobs all to pass.
9. Append final evidence/status to Plan 126 and mark Plan 127 complete.
10. Reconfirm CI if the documentation-only closure commit retriggers it.
11. Merge PR #2 into main.
12. Stop this corrective track.
```

The intended endpoint is a merged `main` with no open correctness/release item from Plans 120–127 and no additional planning artifact required.