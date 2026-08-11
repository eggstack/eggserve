# Plan 115 — Verification, CI, and Test-Surface Consolidation

## Status

**COMPLETE.**

Depends on Plan 112. Prefer executing after Plan 113 so removed product surfaces are not preserved by verification infrastructure.

This phase keeps EggServe well-tested while reducing the cognitive and operational cost of its verification apparatus. The current routine CI shape is already appropriately small; the work here is mainly to simplify ownership, naming, command duplication, and the distinction between routine checks and subsystem diagnostics.

---

## Goal

End with a verification model that can be explained in one short section:

```text
Routine check:
  formatting + lint + normal Rust tests + installed Python wheel test

Release check:
  routine check + supported optional feature/package checks

Subsystem diagnostics:
  fuzz/corpus + filesystem race + fault injection + TLS abuse + proxy interop
  run when the changed subsystem or release risk warrants them
```

No universal deep gate, gate registry, generated evidence system, or CI publication machinery should exist.

---

## Non-goals

Do not:

- reduce correctness coverage merely to lower test count;
- delete security tests that cover a unique hardened invariant;
- put fuzzing back into routine CI;
- put race stress tests into every PR;
- add a release workflow;
- upload evidence artifacts from normal CI;
- create a test-result database or gate registry;
- introduce third-party CI orchestration;
- add flaky live-internet tests;
- require Caddy/nginx for routine development;
- create a platform matrix larger than the project can maintain.

---

# Track A — Inventory verification by invariant, not by historical plan

Create a temporary working inventory of current checks under these categories:

1. static formatting/lint;
2. core unit/integration correctness;
3. raw-wire HTTP correctness;
4. filesystem confinement/security;
5. runtime lifecycle/resource limits;
6. Python API compatibility;
7. Python installed-wheel behavior;
8. TLS behavior;
9. packaging/release checks;
10. diagnostic/stress/fuzz/interop assets.

Inspect at minimum:

```text
.github/workflows/ci.yml
scripts/verify.sh
scripts/test-python-wheel.sh
scripts/verify-cargo-packages.sh
crates/eggserve-core/tests/
crates/eggserve-bin/tests/
crates/eggserve-python/tests/
fuzz/
conformance/
tests/
architecture/testing-and-conformance.md
docs/fuzzing.md
docs/release-process.md
```

For each test file/suite, record whether it covers a unique invariant or substantially duplicates another suite.

### Important distinction

Duplicate *execution* is a cleanup target. Duplicate *security evidence using a different technique* may be intentional.

For example:

- a deterministic `O_NOFOLLOW` invariant test and a concurrent mutation stress test are not necessarily duplicates;
- five separate tests asserting the same CLI argument error through different historical harnesses probably are.

### Acceptance criteria

- every expensive suite has a named invariant or diagnostic purpose;
- obsolete tests whose only subject was removed in Plan 113 are identified;
- no test is retained merely because an old plan number references it.

---

# Track B — Preserve a small routine CI

The current desired CI shape is two jobs:

```text
rust
python
```

Preserve that structure unless a simpler equivalent exists.

The Rust job should cover:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
```

and the minimum optional feature test necessary to prevent the supported standalone TLS build from silently rotting.

The Python job should execute the installed-wheel test harness rather than only testing source-tree imports.

Do not add audit/deny, fuzz, package publication, proxy interop, artifact size, or release certification to routine CI.

### Command ownership

Avoid maintaining semantically different copies of the same routine commands in multiple places.

Preferred options, in order:

1. CI calls a small repository verification command whose behavior is stable and transparent; or
2. CI contains the commands directly and local documentation names the same commands without another wrapper layer.

Do not create an elaborate script solely to remove four lines from YAML.

If `scripts/verify.sh` remains, ensure CI/local nomenclature matches it. If the script's three-tier model is itself the source of confusion, simplify the script rather than adding more aliases.

### Acceptance criteria

- routine CI remains two comprehensible jobs;
- routine CI finishes without running diagnostic/deep suites;
- no duplicated command path can silently diverge on basic fmt/clippy/test behavior;
- TLS receives a minimal supported-feature regression check;
- Python CI verifies an installed wheel.

---

# Track C — Reduce verification tiers

The existing `fast` / `full` / `deep` terminology may be retained only if each tier remains genuinely useful and unambiguous.

Preferred target is two primary modes plus separately named diagnostics:

```text
check
release-check
```

or equivalent existing names if renaming causes more churn than value.

Routine mode:

- formatting;
- clippy;
- workspace tests;
- optionally Python installed-wheel tests if the local environment has required tooling, or a clear separate Python check command.

Release mode:

- routine checks;
- supported TLS feature tests;
- Python wheel build/install/test;
- Cargo package dry-run;
- manual advisory/license checks as documented prerequisites, not mandatory script-installed tooling unless already available.

Diagnostics should be directly callable by purpose, for example:

```sh
cargo test -p eggserve-core --test filesystem_race_qualification
cargo test -p eggserve-core --test fault_injection
cargo test -p eggserve-core --test stateful_fuzz_replay
cargo test -p eggserve-bin --test tls_abuse --features tls
bash tests/proxy/caddy_interop.sh
```

Do not force every diagnostic through a monolithic `deep` command if that encourages unnecessary all-suite execution.

A `deep` convenience alias may remain if it is cheap to maintain, but documentation must state that it is optional aggregation, not a required gate.

### Acceptance criteria

- a contributor can identify the normal pre-commit/pre-PR check immediately;
- release checks are distinct from subsystem diagnostics;
- diagnostics can be selected individually;
- no plan-specific test sequence is treated as a permanent universal gate.

---

# Track D — Remove verification for deleted surfaces

After Plan 113, delete or rewrite tests that exist solely for removed client/legacy-service behavior.

Examples may include:

- client integration/interop/TLS tests if the client subsystem is removed;
- URL parser fuzz target if it has no remaining non-client owner;
- legacy service adapter tests;
- API snapshot entries for removed experimental surfaces;
- test-only direct-Hyper server helpers that no longer represent production architecture.

Do not delete shared canonical-type tests just because client tests also used those types.

### Acceptance criteria

- removed product surfaces do not leave a permanent test maintenance burden;
- shared HTTP/security primitives retain direct tests;
- test count reduction is traceable to deleted behavior or clear duplication.

---

# Track E — Conformance corpus simplification

Inspect the two shared corpora and their consumers.

Keep a corpus if it has real cross-language or state-machine value:

- canonical Rust/Python behavior parity;
- request body state/policy parity.

Do not create additional corpora.

If a corpus is only a serialized duplicate of straightforward unit-test tables with one consumer after Plan 113, consider folding it back into ordinary tests.

Criteria for retaining a corpus:

```text
2+ independent consumers
or
security/regression replay value that is meaningfully easier to audit as data
```

Do not optimize away a corpus that materially enforces Rust/Python semantic parity.

### Acceptance criteria

- each retained corpus has a current reason;
- no new conformance framework is introduced;
- removed client-only corpus entries/targets are cleaned up.

---

# Track F — Fuzzing and stress assets

Fuzz targets are valuable when they attack parser/state boundaries. Retain targets for:

- request-target parsing;
- percent decoding;
- path components/platform checks;
- range/conditional parsing;
- header/canonical response normalization;
- request-body state transitions;
- bounded directory rendering if it has meaningful parser/state behavior.

Delete targets that only exercise removed subsystems.

Do not require cargo-fuzz in routine CI.

Filesystem race qualification remains manual/targeted because it tests a critical invariant that deterministic unit tests cannot fully exercise under adversarial scheduling.

Fault injection remains manual/targeted where it verifies error isolation and permit/resource release.

Proxy interop remains manual and optional. EggServe is not a proxy and should not require Caddy/nginx just to validate ordinary changes.

### Acceptance criteria

- fuzz/stress suites correspond to current attack surfaces;
- no removed subsystem remains solely to keep its fuzz target alive;
- diagnostic suites are documented as targeted tools.

---

# Track G — Documentation truthfulness

Update verification documentation touched by this plan so it states:

- routine CI is regression screening, not release certification;
- `cargo audit` and `cargo deny` are manual release/security checks unless the workflow actually runs them;
- deep diagnostics are selected by change risk;
- GitHub Actions does not publish;
- installed-wheel tests are authoritative for the Python distribution surface.

Do not perform the full documentation consolidation here; Plan 118 owns deduplication. Make only edits required so changed verification commands are not immediately stale.

---

# Track H — Validation of the simplified apparatus

From a clean tree, run the exact routine path intended for contributors and CI.

Then run the release path once in an environment with required tooling.

Finally, select at least one representative diagnostic from each retained diagnostic category affected by the implementation, rather than invoking every suite automatically.

Record:

- routine commands;
- release commands;
- diagnostics selected and why;
- any suite intentionally not run because the changed code could not affect its invariant.

### Acceptance criteria

- documented routine commands succeed;
- CI YAML matches the intended routine policy;
- release check succeeds where tooling is available;
- diagnostic selection is risk-based and explicit;
- no release/publish action occurs.

---

## Final acceptance criteria

Plan 115 is complete when:

- routine CI remains small and has no unnecessary certification machinery;
- the repository has a clear routine-versus-release verification model;
- expensive suites are targeted diagnostic assets rather than universal gates;
- obsolete tests from removed surfaces are gone;
- retained security tests each correspond to a current invariant;
- conformance/fuzz infrastructure is no larger than its current product surface requires;
- verification documentation accurately describes what CI actually runs;
- no GitHub Actions release automation is introduced.

## Rejection conditions

Reject the implementation if it:

- deletes unique path-confinement or raw-wire correctness coverage simply to reduce suite count;
- adds a new gate registry or evidence framework;
- makes fuzz/race/fault/proxy tests mandatory on every PR;
- reintroduces multi-platform release CI;
- publishes crates or wheels from GitHub Actions;
- keeps tests for deleted products solely because they are already written;
- creates a wrapper-script hierarchy more complex than the commands it replaces.
