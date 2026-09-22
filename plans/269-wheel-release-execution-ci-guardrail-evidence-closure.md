# Plan 269 — Wheel release execution, CI guardrail, and evidence closure

## Purpose

Finish the Plans 263–268 Python wheel-distribution campaign by moving the
remaining release-workflow structural guard into routine CI, executing the full
no-publish release graph on one exact candidate SHA, preserving the generated
wheel/manifest evidence, and closing the planning/docs state only if every
required lane succeeds.

Plan 268's corrective implementation is already present. This plan does not
redesign the release pipeline, add wheel targets, change ABI policy, or change
EggServe behavior. It is the final execution/evidence handoff.

Planning baseline:

```text
45c4231 plans: implement wheel release-pipeline corrective (268 Tracks A-F)
```

## Current state

The baseline has:

- the canonical 10-target `release/wheel-matrix.toml`;
- separate manylinux/musllinux baseline and `--compatibility pypi` controls;
- deferred native qualification for cross-built AArch64 and Windows ARM64;
- separate AArch64 glibc and musl qualification jobs;
- matching ARMv7 glibc/musl QEMU userspaces using `sh`;
- explicit post-publish ARMv7 QEMU/binfmt setup;
- build-once/test-many `cp311-abi3` proof for CPython 3.11–3.15;
- required qualification jobs in `aggregate.needs`;
- `scripts/check-release-workflow.py` plus its mutation self-test;
- green routine Rust/Python/supply-chain CI.

The only implementation-level gap found after Plan 268 is that routine CI's
`Wheel-target authority` step does not yet run
`scripts/check-release-workflow.py`; that structural guard currently runs
only in release preflight.

The mandatory Plan 268 `publish_target=none` closure run has not yet executed
on the corrective release graph.

## Constraints

- No Python or Rust public API change.
- No runtime, protocol, filesystem, TLS, or dependency change.
- No additional wheel target or support-tier promotion.
- Keep normal GIL-enabled CPython on `abi3-py311`.
- Keep the 10 required targets exact; candidates remain non-supported.
- Do not publish to TestPyPI or PyPI in this plan.
- Do not waive a failed required lane.
- Do not call a configured workflow lane "qualified" unless the closure run
  actually executed it successfully.
- Do not rewrite historical evidence; append/correct current closure records.

## Track A — Run the release-workflow guard in routine CI

Files:

- `.github/workflows/ci.yml`
- `AGENTS.md`
- `.opencode/skills/eggserve-dev/SKILL.md` if their CI command inventory
  needs synchronization.

Extend the existing `Wheel-target authority` routine-CI step to run:

```sh
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
```

Rationale: release preflight already fails closed on workflow-structure drift,
but a malformed `needs:` graph, QEMU setup regression, or manylinux-policy
regression should fail on push/PR rather than waiting for a manual release
dispatch.

Do not create a new CI job solely for these checks. They are cheap release
metadata/structure checks and belong with the existing wheel authority step.

## Track B — Freeze one closure candidate SHA

After Track A lands and routine CI passes:

1. Record the exact `main` SHA as the closure candidate.
2. Do not merge/push another change before dispatching the no-publish release
   run unless that change intentionally invalidates the candidate and starts a
   new closure attempt.
3. Dispatch the Release workflow from `main`.
4. Verify the release preflight's recorded source SHA equals the intended
   closure candidate before accepting any result.

If GitHub's dispatch surface cannot target a raw commit SHA, the invariant is
still exact: `main` must point to the recorded candidate at dispatch, and the
workflow's `preflight.outputs.sha` / MANIFEST source line must equal it.

Do not accept a run against an earlier Plan 268 SHA after Track A changes CI;
the final candidate includes the routine-CI guardrail.

## Track C — Execute the mandatory no-publish release run

Dispatch:

```text
Workflow: Release
Ref: main (while main == recorded closure candidate)
publish_target: none
```

The run must execute the complete pre-publication graph:

```text
preflight
   |
 build (10 required wheel targets)
   |
   +--> abi-proof
   |      CPython 3.11
   |      CPython 3.12
   |      CPython 3.13
   |      CPython 3.14
   |      CPython 3.15 current available release/RC
   |
   +--> qualify-aarch64-glibc
   |      native ARM64 Ubuntu
   |      3.11 + 3.15 endpoint lanes
   |
   +--> qualify-aarch64-musl
   |      native ARM64 host + ARM64 Alpine container
   |
   +--> qualify-windows-arm64
          native Windows ARM64
            |
            v
        aggregate
            |
       release-wheel-set
       MANIFEST + SHA-256
```

Because `publish_target=none`:

- `publish-testpypi` must be skipped;
- `publish-pypi` must be skipped;
- `post-publish` must be skipped;
- those skips are expected and are not qualification failures.

The pre-publication build/qualification/aggregate jobs are the closure
evidence.

## Track D — Required lane evidence

The closure record must verify, from the workflow run rather than workflow
configuration alone:

### Build matrix

Exactly 10 required `cp311-abi3` wheel artifacts:

- manylinux x86_64;
- manylinux AArch64;
- manylinux ARMv7;
- musllinux x86_64;
- musllinux AArch64;
- musllinux ARMv7;
- macOS x86_64;
- macOS arm64;
- Windows x86_64;
- Windows ARM64.

Confirm the emitted platform tags agree with `release/wheel-matrix.toml`.

### Stable ABI

The same Linux x86_64 wheel bytes must install and pass the ABI smoke on
CPython 3.11, 3.12, 3.13, 3.14, and the currently available 3.15 interpreter.

Record the exact 3.15 version. If final 3.15 is not yet available through the
pinned setup action, an RC resolved through the temporary prerelease mechanism
is acceptable for this current closure attempt because Plan 268 explicitly
tracks the RC-to-final transition. It must be labeled as RC evidence.

Once final CPython 3.15 becomes available, remove the temporary
`allow-prereleases` setting and re-run the relevant ABI/release qualification
before the first production release that claims final-3.15 evidence. That
later maintenance action does not retroactively invalidate a successful
current infrastructure closure.

### Architecture execution

Verify successful execution for:

- AArch64 glibc on native Ubuntu ARM64;
- AArch64 musl inside a native ARM64 Alpine container;
- ARMv7 glibc under matching ARMv7 QEMU userspace;
- ARMv7 musl under matching ARMv7 Alpine/QEMU userspace;
- Windows ARM64 on the native hosted runner;
- macOS x86_64 and arm64 native build-host smoke;
- Windows x86_64 native build-host smoke;
- Linux x86_64 native build-host smoke.

Do not substitute compile success for runtime smoke.

## Track E — Aggregate artifact inspection

After a successful aggregate job:

1. Download the `release-wheel-set` workflow artifact.
2. Preserve the workflow-generated `MANIFEST`.
3. Verify the MANIFEST source SHA equals the recorded closure candidate.
4. Verify it names exactly the 10 wheel files expected by
   `release/wheel-matrix.toml`.
5. Verify every wheel has a SHA-256 digest.
6. Run or confirm the aggregate
   `scripts/check-release-wheel-set.py ... --version 0.2.0` result.
7. Confirm no generic `linux_*`, per-minor `cp312-cp312`/... artifacts, or
   candidate i686/win32 wheels entered the set.

If a workflow artifact is retained only temporarily by GitHub, the committed
closure record must preserve the wheel names, source SHA, matrix-authority
digest, run URL/ID, and aggregate result so the evidence remains auditable
after artifact expiry.

Do not commit wheel binaries to the repository.

## Track F — Failure handling

Any required failure keeps Plans 263–269 open.

Classify a failed run before changing code:

- **workflow/config defect** — write a narrow corrective plan if the repair is
  non-trivial; do not patch ad hoc;
- **runner/service transient** — retry only when evidence shows the repository
  configuration was correct; record the failed attempt and retry reason;
- **unsupported required target** — narrow/demote the support contract through
  a dedicated plan rather than bypassing the gate;
- **3.15 prerelease availability issue** — resolve according to the explicit
  Plan 268/269 RC-to-final policy, not by removing the 3.15 lane.

A rerun is acceptable closure evidence only when the original failure was
clearly external/transient and the successful attempt runs the same candidate
SHA and unchanged workflow content.

## Track G — Commit the closure evidence

After a successful no-publish run, add a dedicated record:

`release/plan-269-wheel-release-execution-closure.md`

It must contain:

- candidate source SHA;
- routine CI run ID/URL and success state;
- manual Release workflow run ID/URL;
- dispatch input `publish_target=none`;
- exact Rust/maturin/PyO3 versions;
- exact Python versions for ABI proof, including whether 3.15 was RC or final;
- all 10 wheel filenames;
- MANIFEST matrix-authority SHA-256;
- wheel SHA-256 values;
- required job/result table;
- aggregate validator result;
- explicit statement that TestPyPI/PyPI publication and post-publish checks
  were intentionally skipped;
- any retry attempt and reason;
- retained Plan 267 candidate/deferred decisions.

Update `release/plan-264-266-wheel-qualification.md` so the previous
"pending release dispatch" statements point to the completed Plan 269 closure
record instead of remaining stale.

## Track H — Close the planning/docs state

Only after Track G evidence exists:

- mark Plan 268 COMPLETE in `plans/ROADMAP.md`;
- mark Plan 269 COMPLETE;
- mark Plans 264–267 CLOSED through the 268/269 qualification;
- preserve Plan 267's free-threaded/ARMv6/PPC64LE/s390x/RISC-V/i686/win32
  decisions;
- remove language saying the first manual dispatch is pending;
- synchronize `docs/release-process.md` and
  `docs/toolchain-support.md` only where the executed evidence changes or
  confirms wording;
- update `AGENTS.md` / skill CI command inventories for Track A.

Do not turn optional physical Raspberry Pi/Le Potato runs into required
per-release gates; the supported claim remains userspace/architecture-based
with the portable real-SBC harness available for maintainer checks.

## Verification

Before dispatch:

```sh
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Routine CI must pass on the exact candidate.

Then the manual `publish_target=none` Release run must reach a successful
`aggregate` job with every mandatory upstream gate successful.

## Acceptance

Plan 269 and the Plans 263–268 wheel campaign close only when:

- routine CI executes both release-workflow structure checks;
- routine CI is green on the exact closure candidate;
- one manual Release run with `publish_target=none` executes that exact
  candidate;
- all 10 required wheel build lanes succeed;
- CPython 3.11–3.15 ABI proof succeeds on the same `cp311-abi3` artifact;
- AArch64 glibc and musl qualification succeeds in the correct native
  userspaces;
- ARMv7 glibc and musl QEMU smoke succeeds;
- Windows ARM64 native qualification succeeds;
- required qualification failures cannot be bypassed by aggregation;
- aggregate validation succeeds with exactly the 10 required artifacts;
- the MANIFEST and SHA-256 evidence are captured;
- no TestPyPI/PyPI publication occurs;
- a committed Plan 269 closure record records the run and artifact evidence;
- the roadmap/docs no longer describe required release lanes as pending;
- no API, runtime, dependency, security-policy, or support-tier regression was
  introduced.

Until those conditions hold, the wheel-distribution campaign remains
**implemented but not closed**.
