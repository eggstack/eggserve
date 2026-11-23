# Python Facade Milestone 326 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/python-facade/326-free-threaded-cpython-314t-staged-support.md`

Source subsystem roadmap:

- `plans/subsystems/python-facade-roadmap.md` (Milestone 326)

Repository baseline reviewed: `4949a77acfe4114561bd7cfe59a8458ea1983af7` (Plans 320–325 push), working tree.

Implementation commits or pull requests:

- None yet. All changes are in the working tree; this task did not commit or push.

## 1. Executive finding

The free-threaded slice is implemented and locally qualified on macOS arm64.
The extension now states `#[pymodule(gil_used = false)]` explicitly; a
`cp314-cp314t` wheel builds from the unchanged PyO3 feature set and imports
with the GIL still disabled; the full installed-wheel suite (861 tests,
including the new `test_free_threaded.py`) passes on CPython 3.14.5t; and the
wheel matrix, release-set validator, metadata gate, release workflow, and CI
understand exactly two ABI families with matrix-owned interpreters.

The named condition is external evidence: the new `python-free-threaded` CI
job and a release aggregate containing the three `cp314t` wheels have not run
from this environment (no push). No Rust crate API, HTTP, TLS, confinement, or
security behavior changed, and the GIL `cp311-abi3` family is unchanged.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP-A explicit `gil_used = false` declaration | `crates/eggserve-python/src/lib.rs` (`#[pymodule(gil_used = false)]` + comment naming the three targets); free-threaded `cargo check` passed | pass | GIL builds compile unchanged |
| WP-B tunnel contention guard | `server/tunnel_bridge.rs` `recv_in_flight` compare-exchange; explicit `concurrent Tunnel.recv is not supported`; reset on every exit path | pass | race covered by `test_tunnel_recv_contention_is_explicit` |
| WP-B one-shot/iterator contract documented | doc comments on `PyRequestBody`, `PyBodyChunkIterator`, `PyResponse`; `docs/python-api.md` | pass | no signature change |
| WP-C free-threaded test module | `crates/eggserve-python/tests/test_free_threaded.py` (8 tests: fresh-interpreter import, parallel callbacks, serialized mode, concurrent producers, body-read race, iterator race, tunnel race, restart) | pass | skipped on GIL builds |
| WP-D reproducible build + compile gate | `pyproject.toml` `maturin>=1.14.1,<2.0`; `scripts/check-python-free-threaded-compile.sh` | pass | no PyO3 feature change needed |
| WP-E family-aware matrix and gates | `release/wheel-matrix.toml` (13 required entries / 10 platform tags); `scripts/wheel-matrix.py`; `scripts/check-release-wheel-set.py`; `scripts/check-python-release-metadata.py`; `release.yml` | pass | self-tests cover missing family, wrong triple, duplicate pair |
| WP-F free-threaded CI lane | `.github/workflows/ci.yml` `python-free-threaded` job (`3.14t`, maturin 1.14.1, compile gate, harness) | configured, not observed | requires a push |
| WP-G support tier, docs, classifier | `docs/` (toolchain, packaging, contract, process, capability matrix, Python API/compat, threat model, extension contract, API stability), `SECURITY.md`, `CHANGELOG.md`, skill bullet, Beta classifier | pass | 3.14t Beta on three targets only |
| WP-H closure + promotion gate | this record; promotion gate recorded in the plan | partial | promotion decision deferred to post-CI evidence |

## 3. Production implementation evidence

### 3.1 Declaration

`#[pymodule(gil_used = false)]` replaces the implicit PyO3 >= 0.28 default.
The attribute is a no-op on GIL-enabled interpreters and documents the audited
promise for CPython 3.14t on Linux x86_64 (glibc), macOS arm64, and Windows
x86_64. `grep -n "pymodule" crates/eggserve-python/src/lib.rs` shows exactly
one declaration.

### 3.2 Tunnel contention

`PyTunnel` gains a private `recv_in_flight: AtomicBool`. `recv` claims it with
`compare_exchange(false, true, AcqRel, Acquire)` before touching the receiver,
returns `RuntimeError("concurrent Tunnel.recv is not supported")` to a racing
caller, and stores `false` after the inner result (data, EOF, error, or
poisoned/closed lock). No mutex is held across `block_on`.

### 3.3 Packaging authority

Every matrix target now declares `abi_family`, `python_tag`, `abi_tag`,
`interpreter`, and `setup_python`. The three free-threaded entries are
`linux-x86_64-glibc-ft`, `macos-arm64-ft`, and `windows-x86_64-ft`, with
artifacts suffixed `-ft`. Uniqueness and completeness are keyed on
`(platform_tag, abi_family)`. The release workflow takes `--interpreter` from
`matrix.interpreter` and setup-python from `matrix.setup_python`; the `abi-proof`
lane remains GIL-only; a `cp314t`-gated step asserts `sys._is_gil_enabled()`
stays false. `scripts/check-release-workflow.py` enforces all four properties.

### 3.4 CI and harness

A separate `python-free-threaded` job uses `actions/setup-python` `3.14t`,
`maturin==1.14.1`, the compile-check script, and the shared wheel harness. The
harness derives site-packages from `sysconfig.get_paths()["purelib"]` (a
free-threaded interpreter uses `lib/python3.14t/site-packages`) and, when
`Py_GIL_DISABLED` is set, runs the installed wheel under `-X gil=0` with
`-W error::RuntimeWarning` before the suite.

### 3.5 Documentation

All support statements now name the three targets and the Beta tier; no doc
claims 3.13t, 3.15t, `abi3t`, PyPy, ARMv7, Windows arm64, or musl
free-threaded support. The Plan 267 report keeps its historical text and the
additive correction banner added at approval time.

## 4. Verification executed

### Commands run

```bash
# Compile configurations (temp copy; the repository worktree is nested under the
# primary checkout, so Cargo workspace discovery fails for the excluded crate)
PYO3_PYTHON=/Library/Frameworks/Python.framework/Versions/3.14/bin/python3.14 \
  cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
PYTHON=<uv 3.14t> bash scripts/check-python-free-threaded-compile.sh
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings
cargo fmt --all -- --check

# Installed-wheel build, smoke, and full suite on CPython 3.14.5t
PYTHON=<uv 3.14t venv with maturin 1.14.1> bash scripts/test-python-wheel.sh

# Static gates
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/wheel-matrix.py validate
python3 scripts/wheel-matrix.py self-test
python3 scripts/check-release-wheel-set.py --self-test
python3 scripts/check-python-release-metadata.py
python3 scripts/check-release-workflow.py
python3 scripts/check-release-workflow.py --self-test
```

### Results

- GIL `cargo check`: pass (8.17 s, cached).
- Free-threaded compile check: pass (3.06 s, cached); `Py_GIL_DISABLED` confirmed.
- `cargo clippy ... -D warnings`: pass.
- Workspace `cargo fmt --all -- --check`: pass.
- Wheel: `eggserve-0.2.5-cp314-cp314t-macosx_11_0_arm64.whl`; maturin 1.14.1
  warned that `abi3` does not support 3.14t and built version-specifically, as
  expected; wheel composition check passed (no bundled executable).
- Installed-wheel assertions: mypy typing smoke pass; import boundary pass;
  free-threaded GIL assertion pass; canonical Python/CLI/subprocess examples pass.
- Free-threaded full suite: `Ran 861 tests in 124.442s` — `OK` on CPython
  3.14.5t (`3.14.5+freethreaded-macos-aarch64-none`), macOS arm64.
- GIL family (invariant proof): wheel
  `eggserve-0.2.5-cp311-abi3-macosx_11_0_arm64.whl` builds; the same harness
  reports `Ran 861 tests in 122.544s` — `OK (skipped=8)` on CPython 3.14. The
  eight skips are the new free-threaded-only tests, by design; the tag and
  behavior are unchanged.
- Packaging gates: matrix validate + self-test, wheel-set self-test, metadata
  gate, and workflow gate + self-test all pass; both workflow YAML files parse
  (5 CI jobs, 10 release jobs).
- `git diff --check`: clean.
- Repository fast gate (`./scripts/verify.sh fast`): all preceding steps passed,
  then the wrapper failed at the documented pre-existing macOS test
  `application_server_qualification_297::app_bounded_body_accepts_and_over_limit_rejects`
  (`ConnectionReset`, exit 101), exactly as recorded in Plan 325's closure. The
  wrapper aborts there, so the later TLS/examples lanes did not run. This is
  unrelated to Plan 326 (no workspace crate changed).

### Not run

- The three-target release build (`linux-x86_64-glibc-ft`,
  `macos-arm64-ft`, `windows-x86_64-ft`) and any CI workflow run: no push was
  made. The manylinux container interpreter path
  `/opt/python/cp314-cp314t/bin/python` is therefore unverified.
- TSan/soak are optional promotion evidence and were not run.

## 5. Invariant review

| Invariant | Evidence |
|---|---|
| GIL `cp311-abi3` tags/count/behavior unchanged | Matrix keeps 10 GIL targets with `python3.11`/`cp311`/`abi3`; metadata gate asserts the declared set; GIL wheel built as `cp311-abi3` and its suite is green (`OK (skipped=8)`) |
| No new `unsafe`; `unsafe_code = "deny"` | Only safe Rust changed; crate lint unchanged |
| Confinement, TLS, framing, crate ownership untouched | No changes outside `crates/eggserve-python` source and packaging/docs; topology gate green |
| One-shot bodies/responses; HEAD suppression | Existing tests green; docs state the contract; no body path logic changed |
| No lock held across a blocking wait | Tunnel claim is atomic; receiver mutex is released before `block_on` (unchanged pattern) |
| No new public Python API or config knob | No `#[pyfunction]`/signature/config field added |
| Claims bounded by evidence | Docs and classifier limited to 3.14t Beta on three targets; closure marks local-only evidence |

## 6. Failure and recovery review

- One-shot contention: exactly one `RequestBody.read()` wins; the loser gets
  `RequestBodyConsumedError` (tested). Concurrent iterator pulls raise a
  structural borrow error or `StopIteration`, never a crash (tested).
- Tunnel contention: the racing `recv` fails explicitly; the guard resets, and
  a later call reports the true terminal state (tested after EOF).
- Restart/shutdown: start/stop under a concurrent callback burst is stable
  (tested); existing shutdown/drain suites remain green.
- Producer budget and teardown observation (Plan 322) are unchanged and green
  under 3.14t, including the `b""`-forever and 503-exhaustion tests.

## 7. Migration and compatibility review

Additive family; no source or API change. Existing handlers may now run in
parallel on 3.14t; this is documented, and `max_python_callbacks=1` restores
serialization. Wheel version stays `0.2.5`; publication is a separate manual
action and was not performed. Rollback within a release window is removing the
three `-ft` matrix entries.

## 8. Security review

No new dependency, `unsafe`, policy field, or confinement path. The import
assertion proves the module never asks a free-threaded interpreter to
re-enable the GIL. Handler parallelism is bounded by the existing callback
permit and producer budget; timeouts still close the HTTP request rather than
Python code. No request/response content is logged by the new paths.

## 9. Documentation and operations

Updated: `docs/toolchain-support.md`, `docs/python-packaging.md`,
`docs/release-contract.md`, `docs/release-process.md`,
`docs/library-capability-matrix.md`, `docs/python-api.md`,
`docs/python-http-server-compatibility.md`, `docs/threat-model.md`,
`docs/extension-contract.md`, `docs/api-stability.md`, `SECURITY.md`,
`CHANGELOG.md`, `.opencode/skills/eggserve-dev/SKILL.md`, and the pyproject
classifier. Static guards: family-aware matrix/wheel-set/metadata/workflow
validators and `scripts/check-python-free-threaded-compile.sh`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `python-free-threaded` CI job and the three-target release aggregate have not run (no push) | Continuous proof and the Linux/Windows builds are unconfirmed | Push and require a green CI run; then build/review a release aggregate |
| medium | The manylinux image interpreter path (`/opt/python/cp314-cp314t/bin/python`) was not executed locally | Linux ft build could fail if the image names it differently | Confirm on the first CI/release run; adjust the matrix `interpreter` if needed |
| low | `verify.sh fast` aborts on the documented pre-existing macOS `ConnectionReset` test (Plan 325 closure) | Later wrapper lanes did not run locally; CI runs the same lanes directly | None for this milestone; fix the baseline test separately |
| low | The excluded Python crate has pre-existing rustfmt drift (not introduced here, not a CI gate) | None | Optional cleanup in a separate change |
| low | TSan/soak remain optional promotion evidence | Promotion to Stable needs them | Run before any tier promotion |

## 11. Roadmap disposition

Milestone 326 is conditionally closed. The implementation, local suite, static
gates, and docs are complete. The named condition is observable CI/release
evidence for the three target lanes; promotion from Beta to Stable remains
blocked until the plan's promotion gate (one green required-family release,
unchanged GIL family, adversarial/contention pass, no medium+ finding) is met.

## 12. Registry updates

- `plans/registry.md`: Python facade current milestone and row 326 become
  `conditionally closed`, linking this record.
- `plans/subsystems/python-facade-roadmap.md`: status line, dependency graph,
  and milestone table record the conditional closure.
- Next milestone numbers continue from `327`.
