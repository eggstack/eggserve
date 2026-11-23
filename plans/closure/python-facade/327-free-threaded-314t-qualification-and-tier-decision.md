# Python Facade Milestone 327 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md`

Source subsystem roadmap:

- `plans/subsystems/python-facade-roadmap.md` (Milestone 327)

Repository baseline reviewed: `4949a77acfe4114561bd7cfe59a8458ea1983af7`
plus the Plan 326 change set; executed on branch
`feat/auto-20261116-7cc8b1dc`.

Implementation commits or pull requests:

- `ab418f8` — Plan 326 change set plus this plan's registration.
- `5327704` — contention soak script and three records.
- `fa6980d` — tier-explicit classifier gate with `--self-test`.
- `b6b793b` — macOS arm64 release runners moved to `macos-latest`.
- Pull request https://github.com/eggstack/eggserve/pull/7 (open; this plan
  did not merge it).

## 1. Executive finding

Both of Plan 326's named conditions are observed, so Plan 326 is closed. The
free-threaded family is qualified on all three targets by a green
`python-free-threaded` CI job and a green 13-pair release aggregate produced
with `publish_target: none`, and the contention substitute for TSan passes
three 120-second runs with zero failures.

The promotion gate is met and confirmed: the owner confirmed during execution
that a no-publish aggregate satisfies the gate's release leg. The tier
decision is therefore **promote to Stable**
(`Programming Language :: Python :: Free Threading :: 3 - Stable`), applied as
a metadata-and-documentation change only (section 7). No product behavior,
public API, dependency, platform, or publication change was made.

## 2. Requirement-to-evidence matrix

| Work package | Requirement | Evidence | Result |
|---|---|---|---|
| A | Commit and push the Plan 326 change set; open a PR; observe all CI jobs | commit `ab418f8`; PR #7; CI run 45277923657 (five job groups green) | pass |
| A | The free-threaded job shows the 3.14t interpreter, compile check, installed wheel, GIL-free assertion, and suite count | `version: 3.14 (Py_GIL_DISABLED)`; CPython 3.14.10 x64-freethreaded; `GIL stays disabled on the installed free-threaded wheel`; `Ran 861 tests in 115.579s` — `OK` | pass |
| B | Dispatch `release.yml` on the branch with `publish_target: none`; reach a green aggregate | release run 45283657891; preflight, 13 builds, four qualifiers, `abi-proof`, aggregate all green; publish jobs and `post-publish` skipped | pass |
| B | The aggregate contains exactly the declared set, including the three `-ft` wheels | `Wheels found: 13`; `OK: required target set is exact (13 platform/family pairs)`; `Platform targets found: 10` | pass |
| B | Record the Linux container interpreter actually used | `Found CPython 3.14t at /opt/python/cp314-cp314t/bin/python` in the pinned manylinux_2_17 container | pass |
| B | `abi-proof` still proves the unchanged GIL family | 5/5 lanes (CPython 3.11-3.15), each installing only `eggserve-0.2.5-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | pass |
| C | Plan 326 closure becomes `closed` with both run URLs and no medium-or-higher finding | `plans/closure/python-facade/326-free-threaded-cpython-314t-staged-support.md` | pass |
| D | Stdlib-only soak script with `--self-test` and three records | `scripts/soak-python-free-threaded.py`; 16/16 self-test checks; `plans/closure/python-facade/327-soak-{1,2,3}.json` | pass |
| D | Document the TSan decision | section 5 | pass |
| E | Evaluate all four promotion criteria | section 6 | pass |
| E | Decide the tier and update metadata/docs/gate only for a promotion | section 7 (promoted to Stable after owner confirmation) | pass |
| F | Closure record, registry, roadmap, next number | this record; registry rows 326/327; roadmap; next number `328` | pass |

## 3. Named condition 1 — CI

CI run https://github.com/eggstack/eggserve/actions/runs/45277923657
(head `ab418f8`) — five job groups, all green.

`python-free-threaded` (3m45s):

- `actions/setup-python` `3.14t` resolved CPython 3.14.10 at
  `/opt/hostedtoolcache/Python/3.14.10/x64-freethreaded`.
- The compile check reported `version: 3.14 (Py_GIL_DISABLED)`.
- Maturin built `eggserve-0.2.5-cp314-cp314t-manylinux_2_34_x86_64.whl`
  (the 2_34 tag is the ubuntu-latest host floor; the release container emits
  the declared 2_17 tag).
- The harness reported `GIL stays disabled on the installed free-threaded
  wheel` and `Ran 861 tests in 115.579s` — `OK`.

`python` (3m20s): `eggserve-0.2.5-cp311-abi3-manylinux_2_34_x86_64.whl`;
`Ran 861 tests in 114.955s` — `OK (skipped=8)` (the eight free-threaded-only
tests skip on a GIL build).

`rust` (19m56s), `platform-compile` macOS (45s) and Windows (3m50s), and
`supply-chain` (5m13s) are green.

## 4. Named condition 2 — release aggregate

Release run https://github.com/eggstack/eggserve/actions/runs/45283657891
(head `b6b793b`, `publish_target: none`):

- Preflight green (supply-chain audits, metadata, matrix/wheel-set/workflow
  self-tests).
- All 13 build entries green. The three free-threaded artifacts:
  - `eggserve-0.2.5-cp314-cp314t-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`
    (`Found CPython 3.14t at /opt/python/cp314-cp314t/bin/python`);
  - `eggserve-0.2.5-cp314-cp314t-macosx_11_0_arm64.whl`
    (`Found CPython 3.14t at /Library/Frameworks/PythonT.framework/Versions/3.14/bin/python3.14t`);
  - `eggserve-0.2.5-cp314-cp314t-win_amd64.whl`.
- The four qualifiers (native AArch64 glibc, AArch64 musl in Alpine, Native
  Windows ARM64) are green.
- `abi-proof`: five lanes (CPython 3.11, 3.12, 3.13, 3.14, 3.15) green; every
  lane installs the same
  `eggserve-0.2.5-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl`
  and runs `release_smoke.py` plus `abi_smoke.py`.
- `aggregate`: `Wheels found: 13`, `OK: required target set is exact (13
  platform/family pairs)`, `Platform targets found: 10`. The
  `release-wheel-set` artifact (ID 14871003335) holds the 13 wheels plus
  `MANIFEST`.
- `publish-testpypi`, `publish-pypi`, and `post-publish` are skipped by their
  existing conditions; nothing was published.

### 4.1 First dispatch and the macOS runner corrective

The first dispatch (run 45281528704, head `ab418f8`) failed before aggregate:
both `macos-14` arm64 jobs were cancelled one second after assignment with
`runner_id 0` and no steps, on attempt 1 and again on attempt 2. The other 11
builds succeeded, including the Linux and Windows free-threaded wheels, but
`abi-proof` and `aggregate` were skipped.

This was diagnosed as runner provisioning, not a build defect: the CI
`platform-compile (macos-latest)` job provisioned an arm64 runner in 45 s on
the same day, and the 2026-10-22 release had built macOS arm64 on `macos-14`
successfully. Commit `b6b793b` moved the two arm64 entries (and the
post-publish macOS smoke) to `macos-latest`; both arm64 wheels then built in
the rerun. The platform tag stays pinned by `MACOSX_DEPLOYMENT_TARGET 11.0`
and `platform_tag`, so no support claim changed.

## 5. Contention evidence and the TSan substitute

A TSan-instrumented CPython is not available in this environment, and
Rust-only TSan would not observe Python callback state (the handlers, bodies,
streams, and tunnels that the free-threaded contract is about). Plan 326
permits a documented substitute; this plan's substitute is
`scripts/soak-python-free-threaded.py` plus the existing contention tests.

The script is stdlib-only, uses loopback only, logs no request or response
content, and runs two server phases (a stop/start cycle) of concurrent
buffered POSTs with payload digests, streamed responses with exact byte
verification, one-shot `RequestBody.read()` races, concurrent
`BodyChunkIterator.__next__` races, tunnel upgrades with concurrent
`Tunnel.recv` contention, and client disconnects during a slow stream. It
exits non-zero on any unexpected error, content mismatch, invariant
violation, GIL re-enable, or request-budget shortfall.

| Record | Duration | HTTP requests | Failures | Tunnel cycles | Disconnects | Handler peak | Threads at exit | Peak RSS |
|---|---|---|---|---|---|---|---|---|
| `327-soak-1.json` | 120.852 s | 84,769 | 0 | 582 | 592 | 8 | 1 | 585.6 MB |
| `327-soak-2.json` | 120.820 s | 80,411 | 0 | 576 | 585 | 8 | 1 | 525.7 MB |
| `327-soak-3.json` | 120.771 s | 82,748 | 0 | 579 | 590 | 8 | 1 | 629.9 MB |

All three ran on CPython 3.14.5 freethreading (macOS arm64) with
`--requests 400 --workers 8 --duration 120`, `max_python_callbacks=8`,
`request_body_mode=buffer`, `max_request_body_bytes=1048576`. The handler
peak equals the permit bound in both phases of every run, the GIL is
disabled before and after, and no worker thread survives a phase.
`--self-test` passes 16/16 checks. No timing threshold is asserted.

## 6. Promotion gate evaluation

| Plan 326 criterion | Status | Evidence |
|---|---|---|
| One release with the family `required` and the aggregate green on all three targets | met | release run 45283657891; matrix tier `required`; three `-ft` wheels in the 13-pair aggregate |
| GIL family unchanged across that release | met | `abi-proof` 5/5 on CPython 3.11-3.15 installing one `cp311-abi3` wheel; 10 GIL wheels unchanged |
| Adversarial/contention pass (TSan or documented substitute) | met | three soak runs, zero failures; section 5 |
| No unresolved medium-or-higher finding in the Plan 326 closure | met | both mediums resolved; remaining findings are low (Plan 326 closure section 10) |

The plan additionally requires the owner to confirm that a
`publish_target: none` aggregate satisfies the gate's release leg. The owner
confirmed during execution (questionnaire response, 2026-11-23) that the
no-publish aggregate counts, so all four criteria are met and confirmed.

## 7. Tier decision

**Promote to Stable** (`Programming Language :: Python :: Free Threading ::
3 - Stable`) on Linux x86_64 (glibc), macOS arm64, and Windows x86_64 only.
The owner confirmed that the no-publish aggregate satisfies the gate's release
leg; all four criteria in section 6 are met.

Applied in this change set:

1. `crates/eggserve-python/pyproject.toml`: the tier-3 classifier replaces
   the tier-2 one.
2. `scripts/check-python-release-metadata.py`:
   `EXPECTED_FREE_THREADING_TIER = 3`; the gate requires exactly one
   free-threading classifier at the declared tier, and its `--self-test`
   covers both configurations (10/10 checks).
3. Tier statements updated: `docs/toolchain-support.md` (support table,
   promotion paragraph, PyO3 note, wheel-matrix note, verification table),
   `docs/python-api.md`, `docs/python-http-server-compatibility.md`,
   `docs/python-packaging.md`, `docs/release-contract.md`,
   `docs/release-process.md`, `docs/library-capability-matrix.md`,
   `SECURITY.md`, `CHANGELOG.md`, and the `eggserve-dev` skill bullet.

No runtime, API, dependency, wheel-tag, or platform change; the wheel set and
its tags are identical to the qualified aggregate.

## 8. Invariant review

| Invariant | Evidence |
|---|---|
| No product behavior, API, runtime config, wire format, or dependency change | Only packaging scripts, soak tooling, matrix runner labels, plans, and one test-free gate script changed |
| GIL `cp311-abi3` family unchanged | 10 GIL wheels in the aggregate; `abi-proof` 5/5; CI GIL suite `OK (skipped=8)` |
| No publication, version bump, tag, or environment approval | `publish_target: none`; publish and post-publish jobs skipped |
| Every support statement backed by an observed run | sections 3, 4, 6; local soak labelled local |
| No claim outside 3.14t on the three targets | matrix still declares exactly three `cp314t` entries; metadata gate green |
| `unsafe_code = "deny"`, confinement, TLS, framing, topology untouched | no Rust source changed; topology gate green in CI |
| A lane failure was fixed only with matrix/workflow wiring | `b6b793b` changes runner labels only |

## 9. Failure and recovery review

- **CI lane failures:** none. All five job groups were green on the first
  run.
- **Release lane failure:** the macOS arm64 jobs were cancelled by runner
  provisioning on two attempts. Diagnosis used the runner id, the step list,
  the successful `macos-latest` CI lane, and the 2026-10-22 release history;
  the corrective changed only runner labels, and the whole workflow was
  redispatched (never a hand-assembled aggregate).
- **Soak failures:** none. The three runs are from clean processes; no run
  was shortened or reduced after a failure.
- **Cancellation and restart:** each soak phase stops its server in a
  `finally` block; the second phase is the stop/start cycle, and no worker
  thread survives.
- **Stale evidence:** the first dispatch (45281528704) is recorded as an
  infrastructure failure, not as aggregate evidence; only 45283657891 is
  cited for the aggregate.

## 10. Residual findings

| Severity | Finding | Status | Note |
|---|---|---|---|
| low | `.github/workflows/platform-qualification.yml` still targets `macos-14` | open | Manual workflow, outside this plan; refresh before its next dispatch |
| low | `./scripts/verify.sh fast` aborts on the pre-existing macOS `ConnectionReset` test (Plan 325 closure) | open | Unchanged; CI runs the same lanes directly |
| low | The excluded Python crate has pre-existing rustfmt drift | open | Not a CI gate |

No medium-or-higher finding remains.

## 11. Registry updates

- `plans/registry.md`: Python facade current-milestone note and rows 326 and
  327 become `closed`; next number stays `328`.
- `plans/subsystems/python-facade-roadmap.md`: status line, dependency graph,
  milestone sections, and the milestone table record both closures.
- Plan 327's header becomes `implemented` with this closure linked.
