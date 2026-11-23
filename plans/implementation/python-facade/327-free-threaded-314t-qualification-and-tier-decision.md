# Python Facade Milestone 327 — Free-threaded 3.14t qualification evidence and Beta tier decision

Status: implemented (closure closed 2026-11-23: both named conditions are
observed, the contention soak passes three runs, the promotion gate is met,
and the owner confirmed the aggregate-as-release reading, so the family is
promoted to Stable; see
`plans/closure/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md`)

Approval: repository owner, 2026-11-23. Approved as drafted; no scope
change from the reviewed plan.

Plan number: 327

Repository baseline: `4949a77acfe4114561bd7cfe59a8458ea1983af7` plus the
uncommitted Plan 326 change set (and this plan's registration). Execution
starts only after that change set is committed and the branch is pushed;
record the resulting SHA at handoff.

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md` (Milestone 327)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (Python `http.server`-shaped
  facade; bounded resources; support claims must match evidence)
- `plans/001-terminology-and-domain-model.md` (verification vocabulary)
- `plans/002-long-term-roadmap.md` (phase 4, Python distribution)
- `plans/003-planning-process.md` §6 and §10 (handoff and planning review)
- `docs/toolchain-support.md`, `docs/release-process.md`,
  `docs/python-packaging.md`

Applicable ADRs:

- none (no architecture or ownership decision; this plan gathers evidence and
  may change a support-tier claim only)

Primary class: infrastructure (qualification evidence and support-tier
decision) with invariant obligations (no product behavior change, the GIL
family unchanged, and no publication)

## 1. Objective

Turn the conditional closure of Plan 326 into an evidence-backed decision.
Two things must happen in order:

1. **Close Plan 326.** Obtain the two conditions its closure record names — a
   green `python-free-threaded` CI job and a three-target release aggregate
   containing the `cp314t` wheels — then update the closure record, registry,
   and roadmap from `conditionally closed` to `closed`.
2. **Decide the Beta tier.** Evaluate Plan 326's promotion gate. Add only the
   missing evidence (a documented substitute for TSan: a bounded contention
   soak), then either promote the family to Stable with the classifier and doc
   changes, or retain Beta and record exactly why.

The plan changes no product behavior in its evidence stage. A support-tier
change is conditional on all four promotion criteria and is the only
packaging change authorized here.

### Decision being executed

- **Evidence before claims.** No document, registry row, or classifier moves
  ahead of an observed run. Run URLs and wheel filenames are the authority.
- **No publication.** The release workflow is dispatched with
  `publish_target: none`; `publish-testpypi`, `publish-pypi`, and
  `post-publish` are skipped by their existing conditions. Cutting a release
  remains a separate owner action.
- **Aggregate counts as the release leg.** Plan 326's gate says one release in
  which the family is required and the aggregate is green on all three
  targets. A `publish_target: none` run produces exactly that aggregate. The
  owner confirms this reading at the promotion decision; if a published
  release is additionally required, this plan stops before the tier change.
- **TSan substitute is explicit.** A TSan-instrumented CPython is not
  available in this environment, and Rust-only TSan would not cover Python
  callback state. Plan 326 permits a documented substitute: a bounded soak
  with concurrent callbacks, streams, and tunnels.

## 2. Why this milestone is ready

- Plan 326 is implemented and conditionally closed
  (`plans/closure/python-facade/326-free-threaded-cpython-314t-staged-support.md`).
  The declaration, family-aware matrix/validators/workflows, CI lane, tests,
  and documentation are in the working tree.
- Local evidence already exists: the free-threaded wheel builds and 861 tests
  pass on CPython 3.14.5t, the GIL wheel stays `cp311-abi3` with 861 tests
  passing (`skipped=8`), the compile gates and clippy pass, and every
  packaging/workflow gate and self-test passes.
- The repository is public, `main` is unprotected, `gh` is authenticated with
  `repo`/`workflow` scopes, and the branch already tracks its remote. The
  branch has no open PR; the last CI runs belong to merged PR #5.
- The CI trigger is known: `push: [main]` and `pull_request: [main]`, so a PR
  is required to exercise the new job. The release trigger is
  `workflow_dispatch` with a `none` mode that skips publication.
- The pypa manylinux image documentation confirms CPython 3.14t lives at
  `/opt/python/cp314-cp314t`, matching the matrix entry. `setup-python`
  documents the `-t` free-threaded version form. Both remain first-run
  confirmations, not blockers.
- No hard dependency is open: Plan 326's own implementation is the input, and
  the only sequencing requirement is committing it first.

## 3. Current implementation evidence

### 3.1 Plan 326 state

| Item | State |
|---|---|
| Plan 326 implementation | all work packages A–G landed in the working tree |
| Plan 326 closure | `conditionally closed`, two medium findings (CI lane not observed; manylinux interpreter path unverified) |
| Local free-threaded suite | `Ran 861 tests ... OK` on CPython 3.14.5t, macOS arm64 |
| Local GIL suite | `Ran 861 tests ... OK (skipped=8)`, wheel `cp311-abi3` |
| Packaging gates | matrix validate/self-test, wheel-set self-test, metadata, workflow gate/self-test: pass |
| Commit | none; the Plan 326 change set (30 modified, 4 new files) is uncommitted, plus this plan and its registration |

### 3.2 Missing evidence

- No CI run for the change set (no commit, no PR). The new
  `python-free-threaded` job has never executed.
- No release aggregate containing the three `cp314t` wheels. The Linux,
  macOS arm64, and Windows x86_64 free-threaded builds have never run on
  their real release targets.
- No soak or TSan evidence. The free-threaded test module proves specific
  contention behaviors, not sustained concurrent load.
- Plan 326's promotion gate is unevaluated; the family is documented as Beta.

### 3.3 Promotion gate as written in Plan 326

| Criterion | Current status |
|---|---|
| One release with the family `required` and the aggregate green on all three targets | not met (no run) |
| GIL family unchanged across that release | locally proven; needs the same run |
| Adversarial/contention pass (TSan or documented substitute) | not met (no soak) |
| No unresolved medium-or-higher finding in the closure | not met (two mediums) |

## 4. Invariants that must not regress

1. No product behavior, public API, runtime configuration, wire format, or
   dependency change in the evidence stage.
2. The GIL `cp311-abi3` family keeps its tags, wheel count, version, and
   suite behavior.
3. No publication, no version bump, no tag, and no PyPI environment approval
   is part of this plan.
4. Every support or tier statement is backed by an observed run URL; local
   evidence is labelled local.
5. No free-threaded claim outside CPython 3.14t on Linux x86_64 (glibc),
   macOS arm64, and Windows x86_64.
6. `unsafe_code = "deny"`, confinement, TLS identity, framing ownership, and
   crate topology stay untouched.
7. A lane failure may be fixed only with packaging, metadata, or matrix
   corrections. Any required product-behavior change stops the plan.

## 5. Scope

### In scope

- Committing and pushing the Plan 326 change set; opening a PR to `main` and
  observing CI.
- Dispatching the release workflow with `publish_target: none` on the feature
  branch and observing preflight, the 13 builds, `abi-proof`, the four
  qualifiers, and the aggregate.
- Bounded fixes when a lane fails: matrix interpreter/ABI metadata, workflow
  wiring, validator expectations, or harness assumptions. No behavior change.
- Updating the Plan 326 closure record to `closed` with run evidence.
- A stdlib-only soak script for free-threaded contention evidence, three
  recorded runs, and a TSan-feasibility note.
- The promotion decision: either the classifier/doc tier change to Stable, or
  a recorded decision to retain Beta.
- The Plan 327 closure record and registry/roadmap updates.

### Explicitly out of scope

- Publishing to TestPyPI or PyPI; creating tags or GitHub Releases.
- 3.13t, 3.15t, `abi3t`, PyPy, ARMv7, Windows arm64, or musl free-threaded
  support.
- New platforms, new runtime knobs, new Python API, or dependency changes.
- TSan as a mandatory gate; performance measurement or claims.
- Any change to Plan 326's historical plan body beyond its status line and
  closure record.

## 6. Required production changes

### Crates and ownership

None in the evidence stage. The soak is a standalone script under `scripts/`;
it consumes only public Python surface and adds no crate dependency. Topology
must stay green.

### Config and policy

None. The release matrix already declares the two families; this plan only
confirms the declared interpreters on real runners.

### Protocol and compatibility

None. No wire or API behavior changes.

### Runtime and concurrency

None. The soak exercises existing bounds (`max_python_callbacks`, the Plan 322
producer budget, one-shot errors, and the tunnel guard) without adding
semantics.

### Frontend or operator surface (CLI / Python)

None in the evidence stage. If the tier is promoted, only the PyPI classifier
and documentation change; no runtime surface changes.

### Security and confinement

None. The soak must not log payload content and must use loopback only.

### Documentation and static guards

- `scripts/check-python-release-metadata.py` must accept exactly one
  free-threading classifier whose tier matches the documented support level;
  today it hard-codes the Beta classifier. This is required so a promotion
  cannot leave metadata and docs out of sync.
- Documentation updates are listed in section 12; only the tier statements
  change, and only after the decision.

## 7. Ordered work packages

### Work package A — Land Plan 326 and observe CI

Intent: produce the first named closure condition with the exact change set
that was locally verified.

Required changes:

- Commit the Plan 326 change set, together with this plan's registration (one
  commit is acceptable; two commits, Plan 326 then Plan 327, are cleaner). Name
  Plan 326 and its evidence in the message. Do not amend or rebase the Plan 325
  history.
- Push the branch and open a PR to `main` (the only trigger that exercises CI
  for a non-`main` branch). The PR body lists the local evidence and the two
  named conditions.
- Observe all five jobs: `rust`, `platform-compile`, `supply-chain`, `python`,
  and `python-free-threaded`.
- If a lane fails, classify the failure before editing: packaging/metadata
  defect (allowed, minimal fix in a follow-up commit) or product behavior
  defect (stop; see section 14).

Acceptance evidence:

- PR URL and exact head SHA.
- Green run URL for the PR, with the `python-free-threaded` job visible.
- The free-threaded job log shows the `3.14t` interpreter, the compile check,
  the installed wheel, the GIL-free assertion, and the full suite count.

### Work package B — Produce the three-target release aggregate

Intent: produce the second closure condition without publishing.

Required changes:

- Dispatch `release.yml` on the feature branch with
  `publish_target: none`. The branch, not `main`, carries the updated
  workflow and matrix.
- Confirm preflight passes (supply-chain, metadata, matrix and workflow
  self-tests), then the 13 build entries, `abi-proof`, the four qualifiers,
  and `aggregate`.
- Confirm the aggregate contains exactly the declared set: 10 GIL wheels plus
  `wheel-manylinux-x86_64-ft`, `wheel-macos-arm64-ft`, and
  `wheel-windows-x86_64-ft`.
- Record the Linux container interpreter actually used; if the pinned image
  exposes a different path, correct only the matrix `interpreter` field and
  rerun.

Acceptance evidence:

- Release run URL and source SHA.
- `aggregate` green; validator output proving the exact 13-pair set.
- Artifact listing with the three `-ft` wheels and their platform tags.
- `abi-proof` still installs only the GIL wheel across CPython 3.11–3.15.

### Work package C — Close Plan 326

Intent: replace the conditional record with the observed evidence.

Required changes:

- Edit `plans/closure/python-facade/326-free-threaded-cpython-314t-staged-support.md`:
  status `conditionally closed` → `closed`; add both run URLs, the wheel list,
  and the resolution of the two medium findings (or a corrected finding if a
  lane needed a fix).
- Update `plans/registry.md` and
  `plans/subsystems/python-facade-roadmap.md`: row 326 and the milestone
  table become `closed`; the current-milestone note cites the closure.

Acceptance evidence: closure record diff, registry/roadmap rows, no remaining
medium-or-higher finding attributable to Plan 326.

### Work package D — Adversarial and contention evidence

Intent: satisfy the promotion gate's evidence leg with a bounded, repeatable
substitute for TSan.

Required changes:

- Add `scripts/soak-python-free-threaded.py` (stdlib only, 3.14t only). It
  starts a `lowlevel.Server` on loopback with `request_body_mode="buffer"`,
  `max_python_callbacks` configurable, and runs a bounded mixed workload:
  concurrent buffered POSTs with a locked counter, concurrent
  `Response.stream` responses with byte verification, repeated tunnel
  upgrades with concurrent `recv` contention, one-shot body/iterator races,
  and at least one server stop/start cycle.
- The script writes one JSON record per run: interpreter build, GIL state,
  configuration, request/worker counts, per-scenario success/failure counts,
  observed max concurrency, peak RSS, final thread count, and duration. It
  exits non-zero on any unexpected error, crash, or content mismatch.
- Provide `--self-test` for its bookkeeping and fixture builders so the script
  is testable without a server.
- Document the TSan decision: no TSan-instrumented CPython is available, and
  Rust-only TSan would not observe Python callback state; the soak plus the
  existing contention tests are the substitute Plan 326 permits.
- Run three repetitions (for example 400 requests / 8 workers / 60–120 s
  each) and retain the JSON records as
  `plans/closure/python-facade/327-soak-{1,2,3}.json` (or as fenced blocks in
  the closure record).

Acceptance evidence: script self-test output, three JSON records with zero
errors, and the TSan assessment paragraph. No timing threshold is asserted.

### Work package E — Evaluate the gate and decide the tier

Intent: make one explicit, evidence-based Beta/Stable decision.

Required changes:

- Evaluate all four Plan 326 criteria against the runs from WP-A/B/D and the
  closed Plan 326 record.
- If all four hold **and** the owner confirms the aggregate-as-release
  reading: change the classifier to
  `Programming Language :: Python :: Free Threading :: 3 - Stable`, update the
  tier statements, and update the metadata gate/self-tests to require the
  Stable classifier for the declared family.
- Otherwise retain the Beta classifier and record the unmet criterion and the
  owner decision. No partial promotion.
- Either outcome updates only packaging metadata and documentation.

Acceptance evidence: a gate matrix with each criterion marked met/not met and
its URL or record; classifier diff (or explicit retain decision); metadata
gate passes in both configurations via its self-tests.

### Work package F — Close Plan 327

Intent: record the decision and leave the control surface truthful.

Required changes:

- Write `plans/closure/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md`
  with the requirement-to-evidence matrix, run URLs, soak records, the
  decision, and any residual findings.
- Update `plans/registry.md` (row 327, current milestone, next number) and the
  roadmap status, graph, and milestone table.

Acceptance evidence: closure record, registry/roadmap diffs, next number
advanced, and no claim stronger than the recorded evidence.

## 8. Failure, cancellation, restart, and contention semantics

- **CI lane failure.** Stop and classify. Packaging/metadata/wiring fixes may
  be made in a follow-up commit and rerun. A discrepancy that requires
  changing bridge behavior or a public contract ends the plan and becomes a
  corrective implementation plan.
- **Release lane failure.** Fix only the matrix `interpreter`/ABI metadata or
  workflow wiring, then rerun the whole workflow. Never hand-assemble an
  aggregate or publish to bypass a failed qualifier.
- **Soak failure.** Any crash, corruption, content mismatch, or unexpected
  exception is a stop condition. Do not reduce the workload until it passes;
  record the failure. If the family cannot be qualified, retaining Beta (or
  opting out with `gil_used = true` in a separate plan) is the fallback.
- **Cancellation and restart.** The soak uses bounded waits and terminates
  its server in `finally`; a cancelled run produces no evidence and is rerun
  from a clean process.
- **Partial or stale evidence.** A run that did not execute the declared
  workload is recorded as not run, never as pass. Evidence from a different
  SHA is labelled and not reused.
- **Concurrent writers.** Only one agent edits the closure/registry surfaces;
  the work packages are ordered to avoid overlapping edits.

## 9. Compatibility and migration

- No product change is planned. The free-threaded consumer contract and wheel
  tags stay exactly as Plan 326 shipped them.
- A promotion changes only the advertised support tier; installing, running,
  and failing over are unchanged. A retain decision changes nothing.
- No semver decision is made here. A published release that carries the
  family remains a separate manual action and is not part of this plan.
- Rollback: a classifier/doc change is a two-line revert; no artifact or
  runtime behavior is affected.

## 10. Required tests

### Focused unit tests

- `scripts/soak-python-free-threaded.py --self-test` (bookkeeping, scenario
  construction, JSON shape).
- `scripts/check-python-release-metadata.py` in both tier configurations
  (Beta and Stable) via fixtures or a self-test.

### Integration tests

- The full free-threaded installed-wheel suite through CI (authority) and
  locally where needed.
- The full GIL suite through CI, proving the unchanged family.

### Restart and recovery tests

- The soak's stop/start cycle, including a shutdown with an idle stream.

### Contention and cancellation tests

- Soak scenarios: concurrent callbacks at the permit bound, concurrent
  producers at the budget, tunnel `recv` contention, one-shot body and
  iterator races, and client disconnects during streams.

### Security and negative tests

- GIL-free import assertion with `-W error::RuntimeWarning` (CI).
- Soak uses loopback only and logs no request or response content.

### Migration and compatibility tests

- Wheel-set validator self-tests and the GIL `abi-proof` lane.

## 11. Required verification commands

```bash
# Work package A
git add -A
git commit -m "Plan 326: free-threaded CPython 3.14t staged support"
git push -u origin feat/auto-20261116-7cc8b1dc
gh pr create --base main --head feat/auto-20261116-7cc8b1dc \
  --title "Plan 326: free-threaded CPython 3.14t staged support"
gh pr checks --watch

# Work package B (must run on the branch that carries the updated workflow)
gh workflow run release.yml --ref feat/auto-20261116-7cc8b1dc \
  -f publish_target=none
gh run list --workflow release.yml --limit 3
gh run watch <run-id> --exit-status

# Work package D
/path/to/python3.14t scripts/soak-python-free-threaded.py --self-test
/path/to/python3.14t scripts/soak-python-free-threaded.py \
  --requests 400 --workers 8 --duration 120 --json /tmp/soak-1.json
# repeat twice; retain all three JSON records

# Static guards (unchanged expectations)
python3 scripts/check-python-release-metadata.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
python3 scripts/check-crate-topology.py
```

Known baseline hazard: on macOS, `./scripts/verify.sh fast` currently aborts
at `application_server_qualification_297::app_bounded_body_accepts_and_over_limit_rejects`
(`ConnectionReset`), as recorded in Plan 325's closure. Record it honestly; do
not change the wrapper. CI runs the same lanes directly.

Excluded-crate Cargo commands from this nested worktree fail workspace
discovery; run them from a standalone copy or worktree (Plan 326 §16).

## 12. Documentation updates

- `plans/closure/python-facade/326-free-threaded-cpython-314t-staged-support.md`
  (status and evidence; no historical rewrite).
- `plans/closure/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md`.
- `plans/registry.md`, `plans/subsystems/python-facade-roadmap.md`.
- If promoted: `docs/toolchain-support.md`, `docs/python-packaging.md`,
  `docs/release-contract.md`, `docs/release-process.md`,
  `docs/library-capability-matrix.md`, `SECURITY.md`, `CHANGELOG.md`,
  `.opencode/skills/eggserve-dev/SKILL.md`, and the pyproject classifier.
- If retained: one sentence in the Plan 327 closure and the docs update
  explaining the unmet criterion; the Beta statements stay.

## 13. Acceptance criteria

1. The Plan 326 change set is committed and pushed; a PR to `main` exists with
   a green CI run, including `python-free-threaded`.
2. A `publish_target: none` release run reaches a green `aggregate` with the
   exact 13 required platform/family pairs, including the three `-ft` wheels.
3. The GIL `abi-proof` lane in that run still proves the unchanged
   `cp311-abi3` family.
4. Plan 326's closure record is `closed` with both run URLs and no unresolved
   medium-or-higher finding attributable to it.
5. The soak script exists, self-tests, and produces three JSON records with
   zero unexpected errors; the TSan-substitute rationale is recorded.
6. The promotion gate is evaluated criterion by criterion; the family is
   either Stable with metadata/docs/gate updated, or Beta with the unmet
   criterion and owner decision recorded.
7. No product behavior, API, dependency, platform, or publication change
   occurred; the GIL family is unchanged.

## 14. Stop conditions

Stop and report rather than improvise when:

- CI or the release aggregate shows a crash, data corruption, or a bridge
  state race under the free-threaded build; the fallback is not a partial
  promotion;
- a lane failure requires changing product behavior, a public contract, or a
  dependency rather than packaging metadata;
- the Linux container interpreter or a target runner cannot provide a 3.14t
  interpreter and no declared alternative exists;
- the aggregate cannot be produced without publishing, or the owner requires a
  published release before promotion (publication is out of scope);
- the GIL family's tags, wheel count, or suite regress;
- the soak cannot be run against a real 3.14t build, or its results
  contradict the existing contention tests;
- an unresolved medium-or-higher finding would remain at the promotion
  decision.

## 15. Closure evidence required

- PR URL, head SHA, and the green CI run URL with all five jobs.
- Release run URL, source SHA, aggregate output, and the artifact list with
  the three `-ft` wheel names and tags.
- The `abi-proof` readout for the unchanged GIL family.
- Three soak JSON records, the self-test output, and the TSan assessment.
- The promotion gate matrix with URLs/records and the owner-confirmed
  decision.
- The device/OS evidence for each target (hosted runner identity is enough).
- Documentation and classifier diffs, or the retain-Beta decision.
- Registry/roadmap rows and the advanced next milestone number.

## 16. Handoff notes

- **CI trigger.** Only `pull_request: [main]` or `push: [main]` runs CI. A
  branch push alone produces nothing; the PR is the mechanism.
- **Release trigger.** Dispatch on the feature branch so the updated
  `release.yml` and matrix are used. `publish_target: none` skips both publish
  jobs and, by their conditions, `post-publish`; no environment approval or
  secret is needed.
- **External visibility.** Commit, push, PR, and workflow dispatch are all
  externally visible; execute them only under the owner's approval of this
  plan.
- **Cost.** CI is roughly 10–15 minutes. The release dry run builds 13 entries
  and runs four native qualifiers; expect 30–60 minutes of runner time.
- **First-run unknowns.** `actions/setup-python` 3.14t resolution on macOS and
  Windows, and the pinned manylinux image interpreter path, are unverified
  until WP-A/B. Both are matrix or workflow metadata, not design changes.
- **Do not publish.** TestPyPI/PyPI, tags, and GitHub Releases are outside
  this plan. If promotion requires a published release, stop and let the owner
  schedule it.
- **No cherry-picked evidence.** A rerun replaces a failed run; never cite an
  earlier SHA's lane as current.
- **Preserve Plan 326.** Only its status line and closure record change; the
  historical plan body and the Plan 267 banner stay intact.
- **macOS fast-gate hazard** and the **nested-worktree Cargo gotcha** from
  Plan 326 §16 still apply.
- **Registry hygiene.** After registering 327, the next-number line must say
  `328`.
