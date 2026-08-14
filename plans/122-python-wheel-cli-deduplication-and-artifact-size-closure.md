# Plan 122 — Python Wheel/CLI Deduplication and Artifact-Size Closure

## Status

**COMPLETE — 2026-08-11.**

Parent roadmap: Plan 120.
Depends on: Plan 121.

Reviewed baseline:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
```

---

## Problem statement

The Python wheel currently contains two native Rust server artifacts:

1. the PyO3 extension `eggserve._native`, which links `eggserve-core`, Tokio, rustls, and the Python server/runtime implementation;
2. a separately built `eggserve-bin` executable staged into `python/eggserve/bin/eggserve[.exe]` and explicitly included by `pyproject.toml`.

`scripts/test-python-wheel.sh` and `.github/workflows/release.yml` both build the standalone distribution binary, copy it into the Python package, then build the native extension. `eggserve._bin._find_binary()` searches for that packaged binary, and `python -m eggserve` launches it as a subprocess.

This preserves CLI parity but duplicates a large fraction of the Rust/TLS stack in one wheel. It also means the Python package's CLI behavior is coupled to finding a second executable even though an equivalent server implementation is already loaded in the extension.

The current `pyproject.toml` does not establish a visible `[project.scripts]` entry in the reviewed baseline; installed-wheel verification exercises `python -m eggserve`, not an actual venv-installed `eggserve` console command. The README nevertheless documents `eggserve` and `pipx run eggserve`. This plan must make that contract explicit and test it from the installed wheel.

---

## Goal

Deliver one Python wheel with one Rust server implementation while retaining all supported user-facing execution modes:

```text
pip/pipx-installed `eggserve` command
python -m eggserve
serve_directory()
ServerProcess
native Python Server / http.server facade
HTTPS classes
```

The standalone Rust CLI remains independently buildable/installable through Cargo. Removing it from the **wheel** does not remove the standalone product.

---

## Supersession note

Plan 119 required “the bundled native CLI remains present in the wheel” because that was the then-current compatibility mechanism. Plans 120/122 supersede only that forward-looking implementation requirement.

Do not edit Plan 119. Preserve its historical closure evidence.

The capability contract remains; the implementation mechanism changes only if installed-wheel parity is proven.

---

## Non-goals

Do not:

- remove the standalone `eggserve-bin` crate;
- remove HTTPS from Python wheels;
- split the package into TLS/non-TLS wheel variants;
- rewrite the entire CLI parser in Python;
- add Click/Typer/argparse frameworks merely to reproduce the Rust CLI;
- add a new workspace crate unless sharing code cannot be achieved cleanly within the existing binary crate;
- introduce automated publishing;
- increase routine CI platform matrices;
- change HTTP/static-serving semantics as part of packaging cleanup.

---

## Track A — Establish artifact and behavior baseline

Before refactoring, produce a baseline from a release-equivalent wheel.

Record:

```text
wheel filename/tag
compressed wheel size
unpacked wheel size
_native extension size
bundled eggserve executable size
other native/shared-library payloads, if any
```

Use wheel ZIP contents rather than relying only on final compressed size. Record the largest members and their percentage of unpacked/compressed payload where practical.

Also verify and record current behavior from a clean venv outside the source tree:

```text
python -m eggserve --help
eggserve --help                 # record pass/fail, do not assume
python -m eggserve fixture serving
serve_directory()
ServerProcess.start()/stop()
HTTPS import/constructor surface
```

If the baseline installed `eggserve` command is currently absent, treat that as a packaging/documentation defect to correct rather than silently weakening the README.

### Acceptance criteria

- baseline wheel is built with the normal dist profile and current abi3 configuration;
- component sizes are recorded in the Plan 122 closure evidence;
- baseline command availability is tested from an isolated installed wheel;
- no source-tree imports or PATH-provided Cargo binary are allowed to mask wheel behavior.

---

## Track B — Share the Rust CLI implementation instead of packaging a second executable

Preferred architecture:

1. Keep `eggserve-bin` as the standalone Cargo package.
2. Refactor its CLI parsing/execution into a reusable Rust library target/module inside the existing package when feasible.
3. Keep `main.rs` as a thin process wrapper around that reusable implementation.
4. Link/reuse that CLI implementation from `eggserve-python` and expose a narrow native function for Python entry-point execution.
5. The shared function returns a result/exit status rather than calling `std::process::exit()` internally.
6. Python `_bin.main()` forwards `sys.argv[1:]` to the native CLI runner and translates the returned status to Python process exit semantics.

This avoids source duplication while ensuring the wheel links only one copy of `eggserve-core`/Tokio/rustls in its extension.

If `eggserve-bin` cannot cleanly expose a library target without creating dependency cycles, a small internal CLI module may move to an existing suitable crate. Do **not** create a generic “CLI framework” abstraction. The shared surface should exist only to support the standalone main and Python launcher.

### Acceptance criteria

- standalone `cargo run/build -p eggserve-bin` still uses the same parser/execution path as the Python entry point;
- there is one source of truth for CLI option parsing and validation;
- Python does not independently duplicate the full Rust option table;
- shared native CLI code returns errors/status rather than terminating the embedding Python interpreter from Rust;
- no dependency cycle is introduced;
- `eggserve-core` does not acquire Python-specific code.

---

## Track C — Replace bundled-binary launch with explicit Python entry points

Update packaging so the wheel does not include:

```text
python/eggserve/bin/eggserve
python/eggserve/bin/eggserve.exe
```

Remove the maturin `include` entries for these packaged executables and the staging steps from wheel-build/release scripts.

Add/verify an explicit console entry point, normally equivalent to:

```toml
[project.scripts]
eggserve = "eggserve._bin:main"
```

The exact target may differ if a cleaner module boundary is chosen, but it must create a real installed `eggserve` command.

`python -m eggserve` must invoke the same entry-point logic.

`_find_binary()` should be removed if no supported API still needs a native executable path. Do not retain dead PATH-search/fallback logic merely for historical compatibility unless it is documented public API.

### Acceptance criteria

- wheel contains no staged standalone `eggserve[.exe]` payload;
- `pip install <wheel>` creates a working `eggserve` command in the environment;
- `eggserve --help` and `python -m eggserve --help` are equivalent in option/exit behavior;
- command execution uses the installed extension, not an unrelated `eggserve` from PATH;
- wheel tests fail if the package accidentally falls back to a system/Cargo binary.

---

## Track D — Preserve subprocess semantics without a bundled binary

`ServerProcess` is explicitly a subprocess lifecycle API. Preserve that isolation.

Instead of resolving a packaged executable, launch the installed module through the current interpreter, conceptually:

```python
[sys.executable, "-m", "eggserve", *_config_to_argv(config)]
```

This creates a genuine child process while using the same wheel/extension and avoids requiring a second native executable.

Review signal/termination behavior on POSIX and Windows. `ServerProcess.stop()` currently terminates the child process; it does not promise the standalone Rust CLI's internal graceful-shutdown signal semantics. Preserve documented behavior and tests rather than inventing cross-platform process supervision.

`serve_directory()` may continue to use `ServerProcess` if that is the established API contract. Do not silently change it into an in-process server if callers may rely on process isolation.

### Acceptance criteria

- `ServerProcess.start()` launches a child process from the same Python environment that imported EggServe;
- child PID differs from parent and `is_running`/`pid` semantics remain correct;
- `stop()` and `wait()` retain current contract on Linux/macOS/Windows-supported paths;
- no PATH lookup is required;
- subprocess fixture serving succeeds from an installed wheel;
- no shell invocation/string command construction is used.

---

## Track E — Preserve full CLI behavior

The deduplicated Python CLI must continue to support the existing documented option surface, including at minimum:

```text
--directory
--addr / --bind / --port
--public
--directory-listing
--follow-symlinks
--allow-dotfiles
--log-format / --quiet
--max-connections
--max-file-streams
--header-timeout
--connection-total-timeout
--handler-timeout
--body-read-timeout
TLS certificate/key options
```

Do not reduce the Python-installed CLI to the smaller `ServeConfig` subprocess option set. The installed command is the product CLI and must retain standalone CLI parity.

Check error messages/exit codes for representative invalid arguments, wildcard bind without `--public`, missing directory, and incomplete TLS configuration.

### Acceptance criteria

- CLI help contains the same supported options in standalone and Python-installed forms;
- representative valid configurations produce the same runtime configuration;
- representative invalid configurations return nonzero and fail closed;
- wildcard/public gating remains identical;
- TLS options remain functional in the wheel;
- no feature reduction is accepted as a size optimization.

---

## Track F — Rebuild and quantify size reduction

After deduplication, build the same release-equivalent wheel and record the same measurements from Track A.

Required evidence table:

```text
metric                         before        after       delta
wheel compressed bytes
wheel unpacked bytes
_native bytes
bundled executable bytes
largest remaining native member
```

The primary acceptance criterion is architectural rather than an arbitrary percentage: the second native executable is gone and no duplicate replacement of similar size was introduced.

If compressed wheel size does not fall materially despite removing the executable, inspect wheel contents and linker behavior before claiming success. Do not start a new dependency-removal project inside this plan.

### Acceptance criteria

- wheel compressed and unpacked sizes are recorded before/after;
- bundled executable contribution falls to zero;
- `_native` growth is explained by shared CLI code and is substantially smaller than carrying a second full executable;
- no other duplicate full server artifact replaces it;
- artifact-size evidence is appended to this plan or a directly linked closure record, not maintained as a permanent benchmark database.

---

## Track G — Update verification/release scripts without growing CI

Update:

```text
scripts/test-python-wheel.sh
.github/workflows/release.yml
scripts/release_smoke.py if its interface assumes a filesystem binary path
relevant packaging docs
```

Remove:

- `cargo build -p eggserve-bin` solely for wheel staging;
- package `bin/` staging/copy cleanup;
- smoke assertions whose only purpose is `_find_binary()`.

Add installed-wheel assertions for:

```text
eggserve --help
python -m eggserve --help
static fixture via installed command
static fixture via python -m eggserve
ServerProcess fixture
```

Reuse the existing routine Python wheel job. Do not add another CI job just for packaging deduplication.

### Acceptance criteria

- routine CI job count is unchanged;
- release workflow no longer stages a second executable into wheels;
- standalone Rust CLI is still built/tested through existing Rust verification where appropriate;
- installed-wheel checks execute from the venv and cannot resolve a source-tree or Cargo PATH artifact.

---

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Additionally run a release-equivalent wheel composition/size measurement and an isolated venv command smoke test.

---

## Explicit acceptance criteria

Plan 122 is complete only when:

- [ ] baseline wheel/component sizes are recorded;
- [ ] actual installed `eggserve` command behavior is baselined;
- [ ] the Python wheel no longer contains a standalone `eggserve[.exe]` server binary, unless a documented blocker proves functionality cannot be preserved without disproportionate complexity;
- [ ] there is one Rust source of truth for CLI parsing/execution;
- [ ] standalone Cargo CLI remains independently functional;
- [ ] installed `eggserve` command works from a clean venv;
- [ ] `python -m eggserve` works from a clean venv;
- [ ] `ServerProcess` still launches a real child process without PATH lookup;
- [ ] `serve_directory()` retains documented behavior;
- [ ] HTTPS classes and CLI TLS options remain available;
- [ ] CPython 3.11 abi3 packaging remains intact;
- [ ] before/after wheel measurements show removal of the duplicate artifact;
- [ ] routine CI remains the existing small shape;
- [ ] full verification passes.

---

## Rejection conditions

Reject the implementation if it:

- removes CLI/subprocess functionality to save bytes;
- replaces the Rust CLI with a second independently maintained Python parser;
- relies on an arbitrary `eggserve` executable from PATH;
- keeps a hidden duplicate native server artifact under another name;
- moves process-exit behavior into a reusable native function in a way that can terminate embedding Python unexpectedly;
- removes TLS or narrows Python versions;
- adds a new wheel flavor/matrix or publishing pipeline;
- turns binary-size measurement into a permanent per-commit benchmark gate.

---

## Closure evidence

### Size reduction (Track F)

```text
metric                         before        after         delta
wheel compressed bytes         1,574,543     1,203,519     -371,024 (-23.6%)
wheel unpacked bytes           3,279,593     2,511,696     -767,897 (-23.4%)
_native.abi3.so bytes          2,260,320     2,348,712     +88,392 (+3.9%)
bundled executable bytes       857,224       0             -857,224 (-100%)
```

The `_native` extension grew by ~88 KB due to linked CLI code, but the bundled
binary is eliminated. Net wheel is 371 KB smaller compressed.

### Acceptance criteria verification

- [x] baseline wheel/component sizes are recorded;
- [x] actual installed `eggserve` command behavior is baselined;
- [x] the Python wheel no longer contains a standalone `eggserve[.exe]` server binary;
- [x] there is one Rust source of truth for CLI parsing/execution (`eggserve-bin/src/lib.rs::run_cli`);
- [x] standalone Cargo CLI remains independently functional (`main.rs` calls `run()` → `run_cli`);
- [x] installed `eggserve` command works from a clean venv;
- [x] `python -m eggserve` works from a clean venv;
- [x] `ServerProcess` still launches a real child process via `sys.executable -m eggserve`;
- [x] `serve_directory()` retains documented behavior (delegates to `ServerProcess`);
- [x] HTTPS classes and CLI TLS options remain available;
- [x] CPython 3.11 abi3 packaging remains intact;
- [x] before/after wheel measurements show removal of the duplicate artifact;
- [x] routine CI remains the existing small shape (2 jobs: rust + python);
- [x] full verification passes.

---

## Plan 126 correction note

The packaging architecture deduplication described above was correct: the
wheel no longer bundles a duplicate `eggserve[.exe]` server binary, and
`eggserve._bin` forwards `python -m eggserve` directly to the
extension-linked CLI. However, the manual `.github/workflows/release.yml`
workflow remained stale until Plan 126: it still built the standalone
binary, copied it into `python/eggserve/bin/`, built the wheel, and
called the now-deleted `eggserve._bin._find_binary()` in its smoke
step. The wheel architecture worked (routine installed-wheel CI stayed
green) but the manual release workflow would have failed on dispatch.

Plan 126 corrected the release workflow to match the deduplicated
architecture, replaced the `_find_binary` smoke with assertions for
`<venv>/bin/eggserve --help`, `<venv>/Scripts/eggserve.exe --help` (on
Windows), and `python -m eggserve --help`, added a wheel composition
assertion that no `eggserve/bin/eggserve[.exe]` exists, and ran the
manual Release workflow successfully across all three platforms.
