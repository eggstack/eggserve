# Plan 114 — Dependency and Artifact Slimming

## Status

**PLANNED.**

Depends on Plan 113's product-surface decisions. Execute measurements against the post-113 tree.

This phase makes dependency ownership truthful and pursues low-risk artifact reductions without reducing supported functionality.

---

## Goal

Reduce unnecessary direct dependencies, accidental feature activation, duplicated runtime ownership, and packaging weight while preserving:

- all supported static-server behavior;
- the Python `http.server` facade;
- optional standalone TLS behavior;
- reusable canonical HTTP/security primitives;
- hardened filesystem semantics;
- current platform support.

Binary-size reduction is a secondary outcome. Manifest correctness and dependency clarity are the primary outcome.

---

## Non-goals

Do not:

- replace Hyper, Tokio, rustls, PyO3, or rustix merely to save bytes;
- introduce custom allocators;
- add linker scripts or platform-specific size hacks unless a measured regression requires them;
- remove TLS functionality solely for size;
- disable error messages, safety checks, or validation to save bytes;
- turn size measurement into a CI gate;
- optimize dev-dependency size;
- remove tests because their dependencies are large;
- chase small compiler-version noise.

---

# Track A — Capture a clean dependency baseline

Record the exact candidate SHA and toolchain.

Run at minimum:

```sh
rustc -Vv
cargo -V
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-bin --features tls
cargo tree -e features -p eggserve-core --no-default-features
```

If Plan 113 retained optional feature sets, record them separately.

For Python:

```sh
cd crates/eggserve-python
cargo tree -e features
```

Classify each direct dependency in every manifest as one of:

- production runtime requirement;
- platform-specific production requirement;
- optional feature requirement;
- build/package requirement;
- test-only requirement;
- unused/redundant direct declaration.

Do not infer from transitive appearance alone. Search actual source ownership.

### Acceptance criteria

- every direct dependency has a classification;
- normal dependencies used only inside `#[cfg(test)]` code are identified;
- optional feature activation is understood before edits;
- Python direct dependencies are compared against functionality already supplied through `eggserve-core`.

---

# Track B — Clean `eggserve-bin` dependency ownership

Current review evidence suggests the binary crate directly declares Hyper-family dependencies even though its production path delegates to `eggserve_core::server` and visible direct Hyper use is concentrated in a test-only legacy helper.

Inspect:

```text
crates/eggserve-bin/Cargo.toml
crates/eggserve-bin/src/*.rs
crates/eggserve-bin/tests/*.rs
```

For each of:

```text
hyper
hyper-util
http-body-util
bytes
```

confirm whether any non-test binary/library code imports it after Plan 113.

Preferred outcomes:

- remove unused declarations entirely;
- move genuinely test-only dependencies to `[dev-dependencies]`;
- retain a normal dependency only when production code directly owns the abstraction.

Do the same audit for Tokio feature flags. Keep only features needed by production binary code plus explicit test-only features where Cargo feature unification does not make the split misleading.

### Acceptance criteria

- the bin manifest no longer claims direct production ownership of libraries used only by tests or core;
- `cargo tree -e features` shows no accidental feature broadening caused by redundant declarations;
- binary behavior remains unchanged.

---

# Track C — Clean `eggserve-core` optional feature ownership

After Plan 113, inspect the remaining features and optional dependencies.

Rules:

- delete feature flags whose complete product surface was removed;
- remove optional dependencies that no remaining feature activates;
- avoid empty compatibility features unless a documented external consumer requires them;
- keep server TLS dependencies feature-gated;
- keep Unix `rustix` platform-gated;
- do not fold platform hardening dependencies into optional convenience features.

Inspect whether historical feature names such as qualification-only Windows flags are still needed for current tests. If they exist only to gate old plan-specific tests, prefer a semantically named test cfg/target organization or removal of obsolete test scaffolding rather than carrying plan numbers in the production manifest indefinitely.

Do not change Windows qualification behavior in this plan if the feature remains necessary for tests that cannot run on non-Windows hosts.

### Acceptance criteria

- no feature points to deleted code;
- no optional dependency is orphaned;
- default core remains minimal;
- TLS remains opt-in for standalone Rust server builds.

---

# Track D — Audit Python extension dependencies

Inspect:

```text
crates/eggserve-python/Cargo.toml
crates/eggserve-python/src/
```

For each direct dependency, determine whether the extension directly uses it or whether it is only needed transitively through `eggserve-core`.

Candidates for careful review include:

```text
tokio
hyper
hyper-util
http-body-util
bytes
futures-util
rustls
```

Do not delete a direct dependency if Python binding code imports its public types or requires its feature activation. But do not preserve duplicate direct ownership merely because the dependency already exists transitively.

This track should coordinate with Plan 117 for the policy question of unconditional TLS. Plan 114 may remove redundant declarations but should not change supported Python TLS behavior before Plan 117 decides that packaging contract.

### Acceptance criteria

- every Python direct dependency has a concrete source-level reason;
- redundant declarations are removed where safe;
- no extension functionality changes unintentionally;
- wheel tests still pass.

---

# Track E — Measure artifacts before and after

Use the existing `dist` profile. Do not create another profile.

Measure default and TLS CLI separately because the same target path is overwritten between builds.

Recommended sequence:

```sh
set -euo pipefail
artifact_dir="$(mktemp -d)"

cargo build --profile dist --locked -p eggserve-bin
cp target/dist/eggserve "$artifact_dir/eggserve-default-dist"

cargo build --profile dist --locked -p eggserve-bin --features tls
cp target/dist/eggserve "$artifact_dir/eggserve-tls-dist"

stat --printf='%n %s\n' "$artifact_dir"/eggserve-*
```

Adapt the filename measurement on non-Linux hosts rather than changing the build profile.

For Python, use the supported wheel build/test path or the documented persistent capture procedure and record:

- native extension uncompressed size;
- bundled CLI uncompressed size;
- compressed wheel size.

Record both before and after values in the implementation closure or `benchmarks/binary-size.md` if that file remains the designated historical snapshot location.

### Interpretation rule

Treat the change as successful if dependency ownership becomes simpler even when size movement is negligible.

A size increase requires explanation if it is larger than ordinary compiler/linker noise. Do not revert a manifest correctness improvement solely because the artifact changes by a trivial amount.

### Acceptance criteria

- measurements distinguish default and TLS builds;
- measurements are reproducible enough to identify material regressions;
- no new permanent size gate is added;
- no claim attributes all size movement to one dependency without evidence.

---

# Track F — Lockfile and feature cleanup

After manifest edits:

```sh
cargo check --workspace
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-bin --features tls
cargo tree -e features -p eggserve-core --no-default-features
```

For Python:

```sh
cd crates/eggserve-python
cargo check
cargo tree -e features
```

Update lockfiles only as generated by Cargo. Do not hand-edit them.

Search for stale dependency-policy prose or feature names:

```sh
rg -n "client-tls|windows-plan086|hyper-util|http-body-util|webpki-roots" \
  Cargo.toml crates docs architecture README.md AGENTS.md
```

Only current active docs need reconciliation here; Plan 118 performs the broader documentation consolidation.

---

# Track G — Verification

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
```

If a dependency removal changes a feature-specific test path, run that feature explicitly.

Do not run all deep verification suites solely because manifests changed unless the change alters filesystem or transport behavior.

---

## Final acceptance criteria

Plan 114 is complete when:

- every direct dependency has an active reason;
- test-only dependencies are demoted where appropriate;
- no deleted feature leaves optional dependencies behind;
- the bin crate no longer redundantly owns production Hyper-family dependencies unless production code actually imports them;
- Python direct dependencies are minimized without changing its supported behavior;
- default/TLS CLI and Python artifact sizes are measured and recorded;
- no supported functionality is removed for size alone;
- no new dependency, build profile, or CI gate is introduced;
- routine Rust, TLS, and installed-wheel verification pass.

## Rejection conditions

Reject the implementation if it:

- weakens hardening or validation for binary size;
- replaces mature core dependencies with bespoke code merely to reduce bytes;
- makes TLS mandatory for the standalone default build;
- removes Python HTTPS behavior before Plan 117 decides that contract;
- adds a binary-size CI threshold;
- introduces fragile platform-specific linker tricks without a concrete need;
- reports a size win without preserving comparable build profiles.
