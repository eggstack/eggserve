# Phase 41 — Final Release-Gate Closure

## Goal

Close the remaining operational and evidence gaps before cutting the first release candidate. The core architecture, HTTP correctness, filesystem safety, Python boundary, client interoperability, packaging, and release workflow are already implemented. This phase must remain narrow: repair release validation, align compatibility claims with actual test coverage, and execute the release path end to end.

This is not a feature phase.

## Starting state

Current release-readiness strengths:

- Rust CI covers Linux, macOS, and Windows.
- Raw-wire, production-path, client, client-TLS, server-TLS, doctest, corpus replay, and Python boundary suites exist.
- Installed-wheel smoke tests clear `PYTHONPATH` and run outside source imports.
- GitHub Actions are SHA-pinned and permissions are explicit.
- Release workflow stages binaries, wheel, checksums, and provenance before publication.
- Stream I/O errors propagate through the HTTP body.
- API stability and release contracts are documented and tested.

Remaining gaps:

1. `cargo audit` and `cargo deny` are invoked without a guaranteed installation step.
2. `eggserve-bin` is built but not package/publish-dry-run validated in normal CI.
3. Python metadata claims `>=3.8`, while routine installed-wheel testing is centered on Python 3.14.
4. Python wheel smoke testing is Linux-only despite Linux/macOS/Windows classifiers.
5. The release workflow needs a verified manual dry-run execution and artifact inspection.
6. CI/release status evidence must be recorded in a release checklist rather than inferred from workflow source alone.

## Track A — Make supply-chain jobs self-contained

Update normal CI and release validation so required cargo subcommands are installed deterministically before use.

Preferred approach:

- install pinned versions of `cargo-audit` and `cargo-deny`;
- use `cargo install --locked`;
- cache installed binaries only if the cache key includes tool name/version and Rust toolchain;
- print tool versions before execution;
- fail if the installed version differs from the declared version.

Do not rely on GitHub-hosted runner preinstallation.

Add a single source of truth for tool versions, for example:

- workflow environment variables;
- a documented table in `docs/dependency-policy.md`; or
- a small checked-in script used by both CI and release workflows.

Required commands:

```sh
cargo audit --version
cargo deny --version
cargo audit
cargo deny check
```

Acceptance:

- a clean runner can execute the supply-chain job without preinstalled cargo plugins;
- normal CI and release validation use the same versions;
- installation is reproducible and documented.

## Track B — Validate both Rust packages as publishable artifacts

Normal CI must validate the exact package graph intended for crates.io.

For `eggserve-core`:

```sh
cargo package -p eggserve-core --locked
cargo publish -p eggserve-core --locked --dry-run
```

For `eggserve-bin`:

```sh
cargo package -p eggserve-bin --locked
cargo publish -p eggserve-bin --locked --dry-run
```

Account for the local path dependency on `eggserve-core`:

- ensure the dependency has an explicit version;
- ensure packaging resolves as crates.io would;
- if bin dry-run requires the core package to exist in a temporary registry/index, document and implement a reliable alternative rather than skipping validation;
- inspect package contents with `cargo package --list` and assert required files are included.

Verify:

- README/license paths are valid from packaged crates;
- no repository-only path is required at build time;
- package metadata is complete;
- `Cargo.lock` policy is intentional;
- the packaged binary crate builds from the generated `.crate` contents.

Acceptance:

- both crates pass package validation;
- both crates pass publish dry-run or an equivalently strong documented local-registry test;
- package contents are reviewed and recorded.

## Track C — Decide and enforce the Python compatibility policy

Make the Python support declaration evidence-based.

### Option 1 — Keep `requires-python = ">=3.8"`

Add a CI matrix covering every supported minor version that is still buildable with the selected PyO3/maturin stack:

- Python 3.8
- Python 3.9
- Python 3.10
- Python 3.11
- Python 3.12
- Python 3.13
- Python 3.14

At minimum, each version must:

- build or install a compatible wheel;
- import all stable public names;
- run API-stability tests;
- run a minimal server callback smoke test;
- run client HTTP smoke tests;
- validate exception types and constructors.

If abi3 is adopted, explicitly configure and document the minimum ABI version. Verify that the produced wheel tags match the intended compatibility model.

### Option 2 — Narrow support

Change `requires-python`, classifiers, README, and packaging docs to the versions actually tested. Do not leave `>=3.8` while testing only 3.14.

Acceptance:

- metadata, documentation, and CI matrix agree;
- the minimum supported Python version is tested;
- the newest supported Python version is tested;
- unsupported versions fail installation cleanly through metadata rather than at native import time.

## Track D — Add cross-platform installed-wheel validation

Run wheel build/install/smoke validation on:

- Linux x86_64;
- macOS runner;
- Windows runner.

Each platform job must:

1. build the wheel using the release-compatible process;
2. create a clean virtual environment outside the source package tree;
3. clear `PYTHONPATH`;
4. install the wheel;
5. verify public imports and package metadata;
6. run static-server and callback-server smoke tests;
7. run low-level client HTTP smoke tests;
8. verify CLI discovery and `python -m eggserve --help`;
9. confirm no native extension is loaded from the checkout.

Account for the documented Windows security position:

- functionality tests are required;
- documentation must continue to state that Windows filesystem confinement is not hardened to the Unix level;
- do not turn a successful wheel smoke test into a stronger security claim.

Where practical, add architecture coverage or use cibuildwheel/maturin matrix tooling for release artifacts. Keep the normal PR matrix bounded enough to remain maintainable.

Acceptance:

- installed-wheel tests pass on all advertised OS families;
- wheel filenames/tags match the platform and architecture;
- CLI and native module discovery are deterministic;
- no source-tree imports occur.

## Track E — Execute the release workflow in dry-run mode

Trigger the manual release workflow with publication disabled.

The dry run must exercise:

- version consistency checks;
- all validation jobs;
- client and server feature matrices;
- supply-chain tooling installation;
- both Rust package validations;
- Python wheel build and installed smoke tests;
- cross-target binary builds;
- artifact upload/download;
- checksum generation;
- provenance generation;
- release bundle assembly;
- publication job gating.

The dry run must not:

- publish to crates.io;
- publish to PyPI;
- create a public GitHub release;
- require production registry tokens.

Inspect the resulting artifact bundle and verify:

- all expected binary archives are present;
- the wheel is present;
- checksums cover every distributable artifact;
- provenance references the correct commit and workflow run;
- no source-tree-only files or stale artifacts are included;
- filenames and target triples are correct;
- archives contain the expected binary, README, and license files.

Record the dry-run workflow URL/run ID and result in the release checklist.

Acceptance:

- the complete validation/build/staging path succeeds with publication disabled;
- artifacts can be downloaded and independently verified;
- publication jobs are provably skipped or blocked;
- any workflow fix triggers another full dry run.

## Track F — Release evidence and checklist closure

Update `docs/release-checklist.md` and `docs/release-criteria.md` with evidence fields rather than generic checkboxes.

Record:

- commit SHA under evaluation;
- CI workflow run IDs;
- dry-run release workflow run ID;
- Rust package/publish-dry-run results;
- Python version matrix result;
- cross-platform wheel result;
- supply-chain audit result;
- corpus replay result;
- latest fuzz campaign date/result;
- known accepted limitations;
- Windows support classification;
- approver/date for RC readiness.

Do not mark a gate complete solely because the workflow YAML contains a job.

Acceptance:

- every release gate points to concrete evidence;
- stale evidence is invalidated by changes to release-critical files;
- RC approval is tied to a specific commit SHA.

## Track G — Documentation and metadata reconciliation

Review and synchronize:

- `README.md`;
- `pyproject.toml`;
- Rust `Cargo.toml` metadata;
- `docs/python-packaging.md`;
- `docs/release-checklist.md`;
- `docs/release-criteria.md`;
- `docs/dependency-policy.md`;
- `docs/action-pinning.md`;
- `SECURITY.md`;
- release workflow comments and inputs.

Specifically verify:

- Python versions and OS classifiers match tested support;
- Rust package names/versions and repository metadata are correct;
- release instructions use the actual workflow inputs;
- dry-run semantics are unambiguous;
- production tokens are required only in the approval-gated publication job;
- Windows hardening limitations remain prominent.

## Required validation matrix

Before marking this phase complete, run:

### Rust

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
cargo test -p eggserve-core --features client
cargo clippy -p eggserve-core --features client-tls --all-targets -- -D warnings
cargo test -p eggserve-core --features client-tls
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-bin --test production_path
cargo test -p eggserve-core --test corpus_replay --features client
```

### Supply chain

```sh
cargo audit
cargo deny check
```

### Packaging

```sh
cargo package -p eggserve-core --locked
cargo publish -p eggserve-core --locked --dry-run
cargo package -p eggserve-bin --locked
cargo publish -p eggserve-bin --locked --dry-run
```

### Python

Run the agreed Python-version matrix and cross-platform installed-wheel smoke suite with `PYTHONPATH` unset.

### Release workflow

Run one complete manual dry run and inspect all artifacts.

## Completion criteria

This phase is complete only when:

- cargo audit/deny jobs install their tools reproducibly;
- both Rust crates are package/publish-dry-run validated;
- Python compatibility metadata matches an executed test matrix;
- installed wheels are validated on Linux, macOS, and Windows;
- the release workflow succeeds end to end in dry-run mode;
- staged artifacts, checksums, and provenance are inspected;
- release evidence is tied to one commit SHA;
- no critical/high release blocker remains.

## Non-goals

- No new HTTP features.
- No ASGI/WSGI adapters.
- No client pooling, redirects, retries, cookies, or proxy support.
- No attempt to claim hardened Windows filesystem confinement.
- No publication of a final release as part of this implementation pass.
