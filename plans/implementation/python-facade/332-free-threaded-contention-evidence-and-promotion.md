# Python Facade Milestone 332 — Free-threaded contention evidence and Beta→Stable promotion

Status: ready for handoff (hard dependency: Milestone 330; soft dependency:
Milestone 331)

Repository baseline: the Milestone 330 merge commit. Plan 330 lands the
free-threaded family as **Beta**; this milestone is the only owner of the
Beta→Stable decision.

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md#7` (milestones)

Long-term requirements:

- `plans/000-long-term-specification.md#2`
- `plans/002-long-term-roadmap.md` (phase 4)

Applicable ADRs: none.

Primary class: infrastructure (qualification evidence) with a documentation
capability outcome (the tier statement).

## 1. Objective

Make the free-threaded support claim repeatable and target-specific, then
decide the tier from that evidence:

1. Extend the contention soak to record provenance (commit, wheel filename
   and tag, host, interpreter, configuration, result).
2. Run it on all three declared targets against the actual `-ft` wheels.
3. Add a bounded contention subset to CI so the claim does not rest on a
   local run.
4. Promote Beta→Stable only if every criterion in Section 13 holds;
   otherwise retain Beta and record the blocker.

No product behavior changes. The only user-facing change is the tier
statement when the evidence supports it.

## 2. Why this milestone is ready

- Milestone 330 lands the wheel family, the `python-free-threaded` CI lane,
  the compile check, and the soak script; this milestone consumes them.
- Milestone 331 closes the concurrency-contract findings; a promotion
  decision must not precede those fixes.
- The branch's own tier record says "wheel filenames are the authority" and
  "no partial promotion"; this milestone supplies exactly that authority.

## 3. Current implementation evidence

### 3.1 What exists after Milestone 330

- `scripts/soak-python-free-threaded.py`: bounded loopback soak; exercises
  buffered POSTs with digests, `Response.stream` byte verification, one-shot
  `RequestBody.read()` races, `BodyChunkIterator.__next__` races, tunnel
  upgrades and `recv` contention, disconnects during slow streams, and a
  stop/start phase.
- CLI: `--requests` (default 400), `--workers` (8), `--duration` (120 s),
  `--callbacks` (8), `--pace-ms` (10), `--max-body-bytes` (1 MiB),
  `--json`.
- Existing records (`plans/closure/python-facade/327-soak-{1,2,3}.json` on
  the adjacent branch, renumbered with its plan): 120 s each, ~80-85k HTTP
  requests, 0 failures, GIL disabled before/after, peak concurrency 8.
- Provenance present: interpreter build string, platform, machine=arm64,
  timestamps, configuration.
- Provenance missing: commit SHA, wheel filename/tag, host identifier,
  runner identity. All three runs are the same local macOS arm64 interpreter
  two minutes apart.
- CI: `python-free-threaded` builds, installs, smokes, and runs the full
  suite on Linux; it does not run the soak.
- Release: the aggregate builds and smokes the three `-ft` wheels; the full
  suite does not run on macOS/Windows.

### 3.2 Why the current evidence is not enough for Stable

The Stable claim covers three operating systems, but contention was measured
on one. The records cannot be tied to a commit or a wheel, so they cannot be
reproduced. The branch's own plan required wheel filenames and no partial
promotion; the records do not meet that bar.

## 4. Invariants that must not regress

- The promotion is all-or-nothing: three targets, one commit, the actual
  released wheel bytes.
- No timing threshold becomes a CI gate; contention failures are invariant,
  content, exception, or GIL-state failures, never raw throughput.
- No publication, tag, or PyPI/crates.io action.
- The GIL family is unchanged and is not re-qualified by this milestone.
- Beta remains the documented tier until the promotion commit lands.
- No new platform, `abi3t`, 3.13t/3.15t, ARMv7, Windows arm64, or musl
  free-threaded target.

## 5. Scope

### In scope

- Soak provenance fields and their validation.
- Three-target soak execution against the `-ft` wheels.
- A bounded contention subset in the `python-free-threaded` CI job.
- A `publish_target: none` release aggregate, with run URL and wheel hashes
  recorded.
- The Beta→Stable decision and its documentation.
- Closing the renumbered branch tier record (334) and updating the registry
  and roadmap.

### Explicitly out of scope

- Concurrency-contract fixes (Milestone 331).
- Any change to the wheel matrix targets, tags, or build model.
- Publication or version bumps.
- Benchmarks, throughput claims, or absolute latency comparisons.
- Windows/macOS full-suite parity (a separate capability decision).

## 6. Required production changes

### Crates and ownership

None. This milestone changes qualification scripts, CI, and documentation.

### Config and policy

None.

### Protocol and compatibility

None; the tier statement is packaging metadata and documentation.

### Runtime and concurrency

No runtime change. The soak asserts existing behavior.

### Frontend or operator surface (CLI / Python)

The classifier in `crates/eggserve-python/pyproject.toml` and the tier text
in `docs/toolchain-support.md` move only if the gate passes.

### Security and confinement

No change. The soak stays loopback-only, logs no payloads, and uses bounded
bodies.

### Documentation and static guards

- `scripts/soak-python-free-threaded.py`: provenance schema and a
  `--require-provenance` validation mode.
- `scripts/check-python-release-metadata.py`: the tier constant remains the
  single declaration; promotion changes it in one place.
- `.github/workflows/ci.yml`: bounded contention step.
- `docs/toolchain-support.md` and every tier-bearing doc.

## 7. Ordered work packages

### Work package A — Provenance-complete records

Intent: make one soak record sufficient to reproduce it.

Required changes:

- Add fields: `commit` (exact SHA), `wheel` (filename and WHEEL tag
  family), `host` (runner name or machine id), `interpreter` (build
  string, `sys.executable`, `Py_GIL_DISABLED`), `started_at`/`ended_at`,
  `config`, and the existing counters.
- Add `--require-provenance` (or equivalent) so a record missing any field
  fails validation.
- Keep `qualified = not problems`.

Acceptance evidence: a record from the local interpreter contains every
field; a record with a missing field fails validation by mutation.

### Work package B — Three-target soak against the wheels

Intent: measure contention on the platforms the tier claims.

Required changes:

- Obtain the three `-ft` wheels from the same commit and record their
  filenames and hashes.
- Run the soak on Linux x86_64 (glibc container), macOS arm64, and Windows
  x86_64 (native runners) against the installed wheel, not a source build.
- Retain at least two records per target (three preferred), 120 s, 8 workers,
  8 callbacks.
- Assert `sys._is_gil_enabled() is False` before and after each run.

Acceptance evidence: six to nine provenance-complete records, all
`qualified: true`, tied to the same commit and wheel bytes.

### Work package C — Bounded CI contention gate

Intent: stop the claim from resting only on manual runs.

Required changes:

- Add a short soak to the `python-free-threaded` job: for example
  `--requests 200 --workers 4 --duration 20 --callbacks 4`, with
  `--require-provenance` supplied from the workflow environment.
- Fail the job on any problem, unexpected exception, content mismatch, or
  GIL re-enable. Do not assert a throughput or latency threshold.
- Keep the full soak manual and documented.

Acceptance evidence: the job fails when the soak is deliberately pointed at a
GIL-enabled interpreter (mutation), and passes on the Beta family.

### Work package D — Release aggregate evidence

Intent: tie the tier to real release artifacts.

Required changes:

- Dispatch the release workflow with `publish_target: none` from the
  promotion commit.
- Record the run URL, the aggregate result, the three `-ft` wheel filenames,
  and their hashes.
- Do not publish.

Acceptance evidence: green aggregate; artifacts match the soak inputs.

### Work package E — Tier decision

Intent: one explicit decision, no partial promotion.

Required changes:

- Evaluate every criterion in Section 13 and tabulate met/not met with
  evidence pointers.
- If all hold: set the classifier to
  `Programming Language :: Python :: Free Threading :: 3 - Stable`, update
  `EXPECTED_FREE_THREADING_TIER`, and update every tier-bearing doc in one
  commit.
- Otherwise: retain Beta, record the unmet criterion, and stop. No partial
  promotion.

Acceptance evidence: the gate table; the classifier diff or the explicit
retain decision; the metadata gate passes in both configurations.

### Work package F — Close and update the control surface

Intent: leave the planning surface truthful.

Required changes:

- Close the renumbered branch qualification record (334) with the new
  evidence; if any criterion is unmet, mark it conditionally closed with the
  blocker.
- Update `plans/registry.md` and `plans/subsystems/python-facade-roadmap.md`
  (status, graph, milestone table, next number).

Acceptance evidence: closure record; registry/roadmap diff; next number
advanced.

## 8. Failure, cancellation, restart, and contention semantics

- Any failed target retains Beta; there is no "two of three" promotion.
- A soak problem is fail-closed: record it, do not rerun until it passes and
  then report only the pass.
- Runner unavailability is a blocker, not a reason to substitute a different
  target or a GIL interpreter.
- The soak's restart phase filters expected "server closed" races; only
  unexpected exceptions count.

## 9. Compatibility and migration

- Beta→Stable is a metadata/documentation change; no install or wire
  behavior changes.
- If Stable is not reached, the Beta statement remains and the blocker is
  recorded; no user action is required.
- Removing the family later requires a new plan; this milestone does not
  create a removal path.

## 10. Required tests

### Focused unit tests

- Soak provenance validation and self-test.
- Metadata gate self-test for both tier constants.

### Integration tests

- Three-target soak against installed wheels.
- Bounded CI contention gate.

### Restart and recovery tests

- The soak's stop/start phase on each target.

### Contention and cancellation tests

- Concurrent callbacks, streams, body reads, iterator pulls, tunnels, and
  disconnect-during-stream on each target.

### Security and negative tests

- A GIL-enabled interpreter must fail the free-threaded lane's assertion.
- A provenance-incomplete record must fail validation.

### Migration and compatibility tests

- The GIL family's wheel set and suite are unchanged.

## 11. Required verification commands

```bash
# Provenance and self-tests
python3 scripts/soak-python-free-threaded.py --self-test
python3 scripts/check-python-release-metadata.py

# Per-target manual soak (run on each target, from the installed wheel)
python3 scripts/soak-python-free-threaded.py \
  --requests 400 --workers 8 --duration 120 --callbacks 8 \
  --require-provenance --json plans/closure/python-facade/332-soak-<target>-<n>.json

# CI-shaped bounded subset (same flags, reduced)
python3 scripts/soak-python-free-threaded.py \
  --requests 200 --workers 4 --duration 20 --callbacks 4 --require-provenance

# Release aggregate (no publication)
gh workflow run release.yml --ref <promotion-sha> -f publish_target=none

# Standard gates after any doc/classifier change
python3 scripts/check-crate-topology.py
python3 scripts/check-release-workflow.py --self-test
PYTHON=python3.14t bash scripts/test-python-wheel.sh
```

Do not claim a command that was not actually run.

## 12. Documentation updates

- `docs/toolchain-support.md` (tier authority; Stable or Beta with the
  blocker).
- `docs/python-packaging.md`, `docs/release-contract.md`,
  `docs/release-process.md`, `docs/library-capability-matrix.md`,
  `docs/python-api.md`.
- `SECURITY.md`, `CHANGELOG.md`, `README.md` where they state the tier.
- `.opencode/skills/eggserve-dev/SKILL.md` and `AGENTS.md` tier lines.
- `plans/registry.md`, `plans/subsystems/python-facade-roadmap.md`,
  closure record 334.

## 13. Promotion criteria (all required)

1. Provenance-complete soak records exist for Linux x86_64 glibc, macOS
   arm64, and Windows x86_64.
2. Every record is `qualified: true` with the GIL disabled before and after.
3. The records use the exact wheels from the `publish_target: none` release
   aggregate of the promotion commit (filenames and hashes match).
4. The bounded contention gate is green in CI on the free-threaded lane.
5. No Medium-or-higher finding is open from Milestone 331 or this milestone.
6. The GIL `cp311-abi3` family is unchanged.
7. No publication is claimed.
8. The tier change and its evidence land in one commit set, with the closure
   record naming the run URL and wheel hashes.

If any criterion is unmet, the tier stays Beta and the closure names the
blocker.

## 14. Stop conditions

Stop and report when:

- a required runner or target is unavailable;
- a target fails the soak and the failure is not understood;
- the wheel bytes tested differ from the aggregate artifacts;
- the promotion would require a code change (that belongs to 331);
- the owner has not confirmed the aggregate-as-release reading for the Stable
  tier;
- the tier change would touch a doc outside the listed set in a way that
  changes unrelated claims.

## 15. Closure evidence required

- The gate table with met/not met and evidence links.
- Provenance-complete records (paths + hashes) for every target.
- The CI run URL for the bounded contention gate.
- The release aggregate run URL and the three wheel filenames/hashes.
- The classifier/doc diff or the explicit retain-Beta decision.
- Registry/roadmap diff and the next-number advance.
- Any residual finding classified by severity.

## 16. Handoff notes

- Run the soak against the installed wheel, not the source tree; a source
  build is not the released artifact.
- Keep the soak loopback-only and payload-free; do not add request logging to
  make failures easier to debug.
- The three targets are the entire claim; do not add a fourth "while we are
  here".
- The branch's original soak records remain trace in the renumbered closure;
  do not delete them. Add the new provenance-complete records alongside.
- If only one target can run, the correct outcome is Beta plus a blocker, not
  a narrowed Stable claim.
