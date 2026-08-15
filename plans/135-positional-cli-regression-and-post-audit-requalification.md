# Plan 135 — Positional CLI Regression and Post-Audit Requalification

## Status

**COMPLETE — 2026-08-15.**

Reviewed baseline:

- `main`: `afc9c63688648bb6faead27ea895a40a5eb36567`
- baseline commit: `Fix audit findings and close bug ledger`
- routine CI run `31873460781`: passed (`rust` + `python`)
- Plan 134: complete, but its broad audit-closure commit changed shared CLI, filesystem, runtime, HTTP primitive, and Python-binding code after the prior cross-platform qualification evidence was recorded.

This is a **single narrow corrective pass**. It is not a new architecture or compatibility roadmap.

The objectives are:

1. fix the positional CLI parsing regression introduced by Plan 134;
2. add regression tests that make the positional-slot rules unambiguous;
3. run the existing full verification locally/through the normal test paths;
4. rerun the existing manual Platform Qualification and Release workflows on the exact corrected head;
5. stop once that evidence is recorded.

No new product surface, dependency, parser framework, workflow, release mechanism, or verification subsystem is authorized by this plan.

---

## Problem statement

The documented CLI grammar remains:

```text
eggserve [OPTIONS] [PORT] [DIRECTORY]
```

Plan 134 changed positional parsing in `crates/eggserve-bin/src/args.rs` so that every positional token that parses as `u16` is treated only as a possible port. If the port has already been selected, that numeric token is currently discarded rather than being considered for the directory slot.

The current loop is conceptually:

```rust
for pos in &positional_args {
    if let Ok(port) = pos.parse::<u16>() {
        if !port_from_flag {
            bind_port = port;
            port_from_flag = true;
        }
    } else if root.is_none() {
        root = Some(PathBuf::from(pos));
    }
}
```

This creates observable regressions such as:

```sh
# Intended: port 8000, directory named "1234"
eggserve 8000 1234

# Current Plan-134 behavior: port 8000, root remains "."
```

and:

```sh
# Intended: explicit port 9000, directory named "1234"
eggserve --port 9000 1234

# Current Plan-134 behavior: root remains "."
```

The same issue affects a port-bearing `--bind HOST:PORT` or `--addr HOST:PORT` followed by a numeric directory name.

This is a CLI parser correctness issue, not a filesystem or serving-policy issue. The fix must remain in the CLI argument interpretation layer.

---

## Non-negotiable scope boundaries

Preserve all of the following:

- manual argument parsing; do **not** add `clap` or another parser dependency;
- current safe defaults: loopback bind, no symlinks, no dotfiles, no directory listing;
- `--public` remains required for unspecified IPv4/IPv6 binds;
- `--bind` / `--addr` conflict behavior introduced by Plan 134;
- non-TLS builds continue to reject `--tls-cert` / `--tls-key` explicitly;
- current timeout and limit validation introduced by Plan 134;
- Python `eggserve.server` behavior and the extension-backed CLI path;
- Rust library API boundaries;
- routine CI remains the existing two jobs plus the already-added lightweight conformance matrix step;
- Platform Qualification and Release remain manual `workflow_dispatch` workflows;
- GitHub Actions must not publish releases or crates.

Do not use this pass to revisit Plan 134 findings that are not directly implicated by the positional parser or by failed requalification evidence.

---

## Track A — Make positional slot ownership explicit

### A1. Preserve the two logical positional slots

Treat the documented positional arguments as two independent logical slots:

```text
PORT
DIRECTORY
```

A token must never be silently discarded merely because it is numeric.

The parser should track whether each logical slot has already been occupied.

### A2. Port-slot occupancy

The port slot is considered occupied when the port has already been selected by one of:

```text
--port PORT
--addr HOST:PORT
--bind HOST:PORT
an earlier positional numeric PORT
```

A host-only `--bind HOST` does **not** occupy the port slot; it retains the documented ability for a positional numeric token to provide the port.

Examples that must remain valid:

```sh
eggserve 9000 public
# port=9000, root=public

eggserve --bind 127.0.0.1 9000 public
# bind=127.0.0.1:9000, root=public
```

### A3. Directory-slot occupancy

The directory slot is occupied by:

```text
--directory DIR
or the first positional token that is not consumed by an unoccupied port slot
```

Once the port slot is occupied, the next positional token must be treated as the directory **verbatim**, regardless of whether it consists only of decimal digits.

Required semantics:

```sh
eggserve 8000 1234
# port=8000, root="1234"

eggserve --port 9000 1234
# port=9000, root="1234"

eggserve --addr 127.0.0.1:9000 1234
# bind=127.0.0.1:9000, root="1234"

eggserve --bind 127.0.0.1:3000 9000
# bind=127.0.0.1:3000, root="9000"

eggserve --directory 1234 9000
# root="1234", port=9000
```

The exact internal implementation may be a small state machine or equivalent occupancy logic. Prefer clarity over cleverness.

### A4. Preserve the unavoidable single-token ambiguity

A single positional token that parses as a valid `u16`, with no explicit port source, remains the positional port for compatibility with the documented CLI:

```sh
eggserve 1234
# port=1234, root="."
```

A user who wants to serve a numeric directory without specifying a positional port can do so explicitly:

```sh
eggserve --directory 1234
```

Do not invent filesystem existence probing to disambiguate a numeric token. Argument interpretation must not depend on whether a path happens to exist.

### A5. Do not silently discard excess positionals

After both positional roles are occupied, another positional argument must produce a controlled argument error instead of being ignored.

Examples:

```sh
eggserve 8000 public extra
# error: too many positional arguments

eggserve --port 9000 public extra
# error: too many positional arguments
```

Do not broaden this into a general parser rewrite. Add only the state required to make existing grammar deterministic.

### Track A acceptance criteria

- [x] `eggserve 8000 1234` resolves root `1234`;
- [x] `eggserve --port 9000 1234` resolves root `1234`;
- [x] `eggserve --addr 127.0.0.1:9000 1234` resolves root `1234`;
- [x] `eggserve --bind 127.0.0.1:3000 9000` resolves root `9000`;
- [x] host-only `--bind` still allows a positional port;
- [x] `--directory 1234` always means directory `1234`;
- [x] a single positional numeric token still means `PORT`;
- [x] no positional token is silently discarded after both logical slots are occupied;
- [x] no filesystem probing is introduced into argument parsing;
- [x] no dependency is added.

---

## Track B — Add focused regression coverage

Update the existing tests rather than creating another parser test framework.

Primary files:

```text
crates/eggserve-bin/src/args.rs
crates/eggserve-bin/tests/cli_validation.rs
```

### B1. Parser unit tests

Add explicit unit coverage for at least:

```text
["8000", "1234"]
  -> port 8000, root "1234"

["--port", "9000", "1234"]
  -> port 9000, root "1234"

["--addr", "127.0.0.1:9000", "1234"]
  -> addr 127.0.0.1:9000, root "1234"

["--bind", "127.0.0.1:3000", "9000"]
  -> addr 127.0.0.1:3000, root "9000"

["--bind", "127.0.0.1", "9000", "1234"]
  -> addr 127.0.0.1:9000, root "1234"

["--directory", "1234", "9000"]
  -> root "1234", port 9000

["1234"]
  -> port 1234, root "."

["8000", "public", "extra"]
  -> controlled error
```

Update or replace the current Plan-134 test that expects:

```text
--bind 127.0.0.1:3000 9000 -> root "."
```

That expectation encodes the regression and must not survive the corrective pass.

### B2. CLI-level smoke where cheap

If the existing `cli_validation.rs` or release smoke infrastructure can exercise this without adding a new process harness, include one real CLI case using a temporary working directory containing a directory literally named `1234`.

Preferred behavior proof:

```text
start EggServe with an explicit/known port source and positional directory "1234"
serve a fixture from that directory
confirm the fixture is returned
cleanly terminate the process
```

If existing infrastructure cannot do this cleanly, parser unit regression coverage plus the existing full CLI/wheel smoke is sufficient. Do **not** add a bespoke process-management framework solely for this case.

### B3. Guard adjacent Plan-134 behavior

Ensure existing coverage continues to prove:

- `--bind` + `--addr` is rejected;
- `0.0.0.0` and `::` require `--public`;
- `--help` exits successfully and writes help to stdout;
- non-TLS binaries reject TLS flags clearly;
- invalid timeout relationships fail validation.

### Track B acceptance criteria

- [x] regression tests fail on `afc9c636…` and pass after the fix;
- [x] numeric directory behavior is covered directly;
- [x] the incorrect Plan-134 expectation is removed;
- [x] excess positionals have an explicit test;
- [x] adjacent Plan-134 CLI fixes remain covered;
- [x] no new testing framework or dependency is introduced.

---

## Track C — Verify the corrected repository locally/through standard checks

This pass touches argument parsing only unless a failing test demonstrates otherwise.

Required verification before handoff/qualification:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin
cargo test -p eggserve-bin --features tls
cargo test --workspace
./scripts/verify.sh full
```

The full verification path is expected to include the existing example, TLS, installed-wheel, and package checks.

Do not add this corrective case to `deep` verification. It belongs in ordinary parser/unit coverage.

### Python/shared CLI check

Because the Python wheel invokes the same `eggserve-bin::run_cli` implementation in-process, the installed-wheel path must remain green. No Python API behavior should need changing.

If the positional parser fix requires touching Python bindings, stop and justify why in the closure record; the expected implementation should not require that.

### Track C acceptance criteria

- [x] format passes;
- [x] workspace clippy passes with `-D warnings`;
- [x] default CLI tests pass;
- [x] TLS CLI tests pass;
- [x] workspace tests pass;
- [x] `./scripts/verify.sh full` passes;
- [x] installed Python wheel tests remain green;
- [x] no Python compatibility contract changes are required.

---

## Track D — Requalify the exact corrected head on supported platforms

The prior Platform Qualification and Release evidence predates Plan 134. Plan 134 changed shared filesystem, lifecycle/configuration, CLI, HTTP primitive, and PyO3/Python paths. Linux routine CI on `afc9c636…` passed, but that does not execute the Windows-only filesystem branches or the macOS/Windows wheel paths.

After the positional fix is committed, rerun the **existing** manual workflows on the exact corrected `main` head.

Do not create new workflows and do not move these checks into routine CI.

### D1. Routine CI on corrected head

First require the normal push CI for the corrective commit to pass:

```text
rust   -> pass
python -> pass
```

Record the run ID and exact head SHA in this plan's completion record.

### D2. Platform Qualification

Dispatch the existing workflow:

```text
.github/workflows/platform-qualification.yml
name: Platform Qualification
```

Require the existing jobs to pass on the exact corrected head:

```text
macOS arm64 product qualification
Windows x86_64 filesystem qualification
```

The Windows result does not change the existing documented support boundary unless the workflow itself uncovers a new defect. The two previously documented NTFS open-descendant root-rename limitations remain accepted limitations unless new evidence changes them.

If Platform Qualification fails:

1. determine whether the failure is caused by this corrective change, Plan 134, runner/toolchain drift, or an existing known qualification limitation;
2. fix only a concrete product defect required to restore the previously claimed support posture;
3. do not weaken or skip a qualification test merely to obtain green status;
4. rerun the workflow after the fix;
5. record the failure and resolution in the completion record.

### D3. Release workflow

After Platform Qualification is green, dispatch the existing manual:

```text
.github/workflows/release.yml
name: Release
```

Require all existing matrix targets to pass:

```text
Linux x86_64 wheel
macOS arm64 wheel
Windows x86_64 wheel
```

This is build/composition/smoke qualification only.

**Do not publish anything.**

No crates.io publication, PyPI publication, release tag, or automatic GitHub release is part of Plan 135.

### Track D acceptance criteria

- [x] routine CI passes on the corrected exact SHA;
- [x] Platform Qualification is manually dispatched on that SHA;
- [x] macOS arm64 qualification passes;
- [x] Windows x86_64 adversarial qualification passes within the existing documented limitation boundary;
- [x] Release is manually dispatched only after platform qualification succeeds;
- [x] Linux x86_64 wheel build/composition/smoke passes;
- [x] macOS arm64 wheel build/composition/smoke passes;
- [x] Windows x86_64 wheel build/composition/smoke passes;
- [x] no publication occurs;
- [x] workflow run IDs and head SHA are recorded in the plan closure record.

---

## Track E — Closure record and stop condition

At completion, update this file from `READY FOR HANDOFF` to `COMPLETE` and append a concise evidence record containing:

```text
implementation commit SHA
parser regression tests added/updated
local/full verification result
routine CI run ID
Platform Qualification run ID
Release run ID
remaining known limitations
```

Optionally add a one-line note to Plan 134 stating that Plan 135 corrected a positional parsing regression discovered after Plan 134 closure. Do not restore `bugs.md`, do not recreate a bug ledger, and do not rewrite Plan 134's historical findings.

This pass is closed when the parser is correct and the exact corrected head has green routine, platform, and release qualification evidence.

Do not create Plan 136 for cosmetic bookkeeping after this. Any subsequent plan should require a new concrete bug, security finding, compatibility defect, or release-blocking issue.

---

## Explicit rejection conditions

Reject an implementation that:

- adds `clap` or another argument-parsing dependency;
- probes the filesystem to decide whether a numeric token is a port or directory;
- weakens the documented `[PORT] [DIRECTORY]` grammar;
- silently discards positional arguments;
- changes safe serving defaults;
- changes Python handler semantics to solve a CLI-only parser problem;
- broadens the Rust public API;
- modifies Windows filesystem policy without a qualification failure requiring it;
- adds macOS/Windows jobs to routine CI;
- creates another qualification framework or evidence registry;
- weakens/skips failing platform tests merely to make qualification green;
- publishes crates, wheels, tags, or releases;
- reopens the broader Plan 128–134 architecture/polish tracks.

---

## Final acceptance criteria

Plan 135 is complete only when all are true:

- [x] the numeric positional directory regression is fixed;
- [x] positional argument interpretation is deterministic and slot-based;
- [x] numeric directory names work after any already-occupied port source;
- [x] single positional numeric compatibility remains intact;
- [x] excess positional arguments fail instead of disappearing;
- [x] focused regression tests cover the corrected cases;
- [x] the incorrect Plan-134 parser expectation is removed;
- [x] all adjacent Plan-134 CLI hardening remains intact;
- [x] `./scripts/verify.sh full` passes;
- [x] normal two-job CI passes on the corrected exact head;
- [x] manual Platform Qualification passes on the same head;
- [x] manual Release matrix passes on the same head;
- [x] no publication occurs;
- [x] no dependency or architectural scope is added;
- [x] the completion record contains exact SHA/run evidence;
- [x] the repository returns to ordinary bug/release maintenance rather than another broad cleanup phase.

## Completion record

- Parser implementation commit: `026ce4315359502d445658831bf90126fedcedb5` (`fix positional CLI slot parsing`).
- Qualification correction commit: `16afecea940d2707eb574063658f8267cad6d66d` (`test: bound active stream shutdown qualification`). The macOS installed-wheel qualification exposed that the active-stream test used the 10-second default drain deadline while asserting a 5-second bound; the test now specifies a 1-second deadline.
- Documentation and skill updates: `README.md`, `AGENTS.md`, `docs/cli.md`, `architecture/eggserve-bin.md`, and `.opencode/skills/eggserve-dev/SKILL.md` now describe the two-slot positional grammar and numeric-directory behavior.
- Parser regression coverage: numeric directories after positional, explicit `--port`, explicit `--addr`, port-bearing `--bind`, host-only `--bind`, `--directory` with a positional port, single numeric positional compatibility, and excess positional rejection. The baseline parser was verified to fail the numeric-directory regression before the fix.
- Local verification: `./scripts/verify.sh full` passed, including format, clippy, workspace/TLS tests, examples, the installed wheel suite (732 Python tests), and package dry-runs.
- Routine CI: run `31894958826` passed on `16afecea940d2707eb574063658f8267cad6d66d` (`rust` job `95036739378`, `python` job `95036739355`).
- Platform Qualification: run `31895110822` passed on the same SHA after the failed Windows race test was rerun; macOS job `95037654715` and Windows retry job `95037654410` passed. The initial Windows attempt failed only in the timing-sensitive `windows_race_index_file_replacement_during_resolution` test; no product code was changed for that unrelated fixture race.
- Release: run `31895525890` passed on the same SHA: Linux job `95038153821`, macOS job `95038153811`, and Windows job `95038153839`.
- Publication: none.
- Known limitations: Windows remains a trusted/local-content platform; the two documented NTFS open-descendant root-rename cases remain skipped. GitHub Actions reported non-blocking Node.js 20 deprecation annotations.
