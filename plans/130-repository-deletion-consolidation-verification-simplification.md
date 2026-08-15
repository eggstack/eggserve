# Plan 130 — Repository Deletion, Consolidation, and Verification Simplification

## Status

**COMPLETE — 2026-08-15.**

Governing roadmap: Plan 128.

Depends on: Plan 129 qualification inventory/evidence where platform-specific scripts or tests may be candidates for retention.

This is a deletion-first maintenance pass. EggServe has accumulated extensive planning, verification, architecture, benchmark, and compatibility machinery during hardening. The implementation is now mature enough that repository weight should be reduced where it no longer protects a live invariant.

The target is not the smallest possible repository. The target is the smallest set of code, tests, scripts, workflows, and normative documentation that still protects EggServe's product/security contract.

---

## Track A — Build a deletion/consolidation inventory

Before deleting anything, classify candidate artifacts into these buckets:

```text
A. production/runtime code
B. public API/package surface
C. routine regression protection
D. manual/deep qualification protection
E. normative documentation
F. historical planning/evidence
G. stale/duplicated/unreferenced artifact
```

Only bucket G is an unconditional deletion candidate. Bucket F should generally remain as history unless it creates active confusion or duplicated generated artifacts; do not rewrite old plans to look current.

Inspect at minimum:

```text
.github/workflows/
scripts/
release/
benchmarks/
conformance/
fuzz/
tests/
docs/
architecture/
crates/*/tests
Cargo.toml feature flags and dev-dependencies
AGENTS.md and agent skill files
```

Use repository search before deletion to identify references. If code search is unavailable, use filesystem/grep locally during implementation.

### Acceptance criteria

- [ ] every deletion candidate has a stated reason;
- [ ] references are checked before deletion;
- [ ] normative docs are distinguished from historical plans;
- [ ] no security corpus is deleted merely because it is not in routine CI;
- [ ] no release/package script is deleted before confirming its current caller.

---

## Track B — Consolidate the manual release workflow

The current `.github/workflows/release.yml` repeats nearly the same build, wheel-composition check, install smoke, and artifact upload logic for Linux, macOS, and Windows.

This duplication has already caused synchronization/YAML defects. Reduce it.

### Preferred design

Use a small matrix for the three existing targets while preserving platform-specific command differences only where necessary.

Conceptual matrix:

```yaml
strategy:
  matrix:
    include:
      - os: ubuntu-latest
        artifact: wheel-linux-x86_64
        python_bin: /tmp/smoke-venv/bin/python
        eggserve_bin: /tmp/smoke-venv/bin/eggserve
        auditwheel: repair
      - os: macos-14
        artifact: wheel-macos-arm64
        ...
      - os: windows-latest
        artifact: wheel-windows-x86_64
        ...
```

Do not force all shell behavior into unreadable matrix expressions. If a small platform-specific step is clearer, keep it. The goal is to deduplicate the invariant logic, not maximize YAML cleverness.

Extract the wheel composition assertion into a tiny repository script only if that meaningfully reduces duplication and gives it a reusable home. Prefer Python stdlib only.

The workflow must remain:

```text
workflow_dispatch only
build/test artifacts only
no PyPI publication
no crates.io publication
no GitHub Release publication
```

### Acceptance criteria

- [ ] release workflow is materially shorter or less duplicated;
- [ ] three existing target platforms remain represented;
- [ ] wheel composition assertion remains on every target;
- [ ] installed `eggserve --help` remains checked;
- [ ] installed `python -m eggserve --help` remains checked;
- [ ] real fixture serving remains checked;
- [ ] release remains manually dispatched;
- [ ] no publishing credentials/actions are added;
- [ ] Windows release smoke remains distinct from Windows adversarial qualification.

---

## Track C — Verify and simplify `scripts/`

For every script, determine:

```text
who calls it?
which verification tier owns it?
can its behavior be expressed by an existing script?
is it historical one-off machinery?
does it duplicate CI YAML?
does it require obsolete tools/dependencies?
```

The intended final hierarchy should be easy to explain:

```text
scripts/verify.sh fast   -> normal Rust developer regression screen
scripts/verify.sh full   -> release-like Rust + Python/package/example checks
scripts/verify.sh deep   -> manually selected expensive/adversarial suites
special-purpose scripts -> only when a named platform/package task is genuinely distinct
```

Prefer deleting scripts that only wrap one obvious command unless they encode meaningful cross-platform behavior or package isolation.

Do not turn `verify.sh` into a monolithic orchestration framework. It should dispatch a small number of obvious command groups.

### Acceptance criteria

- [ ] every remaining script has a documented caller/purpose;
- [ ] obsolete one-off planning scripts are removed;
- [ ] duplicated smoke logic is consolidated where practical;
- [ ] `fast`, `full`, and `deep` remain understandable without reading many helper layers;
- [ ] no new Python package is introduced for verification;
- [ ] routine CI continues to invoke straightforward commands/scripts.

---

## Track D — Remove plan-era implementation leakage

Audit production manifests/code for names that encode historical plan numbers or temporary migration states.

A known candidate is the `windows-plan086` Cargo feature currently declared in `eggserve-core`. Verify whether it is referenced by any production/test/package path. If it is unused, remove it. If it still protects a live conditional path, rename it to a behavior-based feature only if the feature genuinely remains necessary.

Also inspect for:

```text
planNNN identifiers
migration-only adapters
legacy compatibility aliases with no public users
stale TODO/FIXME comments referring to completed plans
dead cfg flags
unused package metadata/workarounds
```

Do not delete public compatibility names solely because internal search finds no repository callers; distinguish public API from internal dead code.

### Acceptance criteria

- [ ] no unused plan-number feature flag remains;
- [ ] plan numbers do not define runtime behavior;
- [ ] completed migration scaffolding is removed when unreferenced;
- [ ] public API removals are not made casually during this cleanup;
- [ ] Cargo feature combinations still build after removals.

---

## Track E — Dev-dependency and benchmark/test support audit

Audit dev-only dependencies independently from runtime dependency slimming.

Candidates such as Criterion, proptest, serde/serde_json, tempfile, rcgen, TLS test dependencies, and libc are acceptable if an active benchmark/test uses them. Remove only unused entries.

For `benchmarks/`, `conformance/`, and `fuzz/`:

- retain corpora/targets that protect HTTP or filesystem invariants;
- remove duplicate baseline snapshots whose result is already summarized normatively and which are not used by tooling;
- avoid deleting reproducibility evidence that is still referenced by a current benchmark plan/document;
- prefer one current benchmark methodology over multiple historical harness variants.

No benchmark needs to become a CI gate.

### Acceptance criteria

- [ ] unused dev-dependencies are removed;
- [ ] active security/property/fuzz dependencies remain;
- [ ] benchmark assets have a clear current-vs-historical distinction;
- [ ] no runtime feature is removed in the name of test slimming;
- [ ] Cargo lockfiles are regenerated only as required by actual manifest changes.

---

## Track F — Documentation consolidation boundaries

This plan may delete obviously duplicated/stale documents, but substantive wording cleanup belongs to Plan 131.

Use this plan to establish document ownership:

```text
README.md                         product overview / quickstarts
docs/security-policy.md           normative safe-default policy
docs/threat-model.md              threat assumptions
docs/python-http-server-compatibility.md  Python compatibility contract
docs/python-api.md                Python API reference
docs/cli.md                       CLI reference
docs/http-primitives.md or equivalent     Rust/public primitive reference
architecture/*                    implementation architecture, not user quickstarts
plans/*                           historical/implementation records
```

Where two normative docs own the same detailed table or invariant, choose one owner and replace the duplicate with a short link/reference in the other.

Do not delete architecture documents solely because they are long. Delete or merge only when they duplicate the same subsystem truth and make maintenance worse.

---

## Track G — Routine CI restraint

The current routine CI is intentionally small. Preserve that.

Do not add:

```text
Windows routine job
macOS routine job
cargo audit on every push
cargo deny on every push
fuzzing on every push
benchmarks on every push
release wheel matrix on every push
coverage gates
artifact/evidence uploads
```

If Plan 129 introduces a manual Windows qualification workflow, keep it manual or remove it after collecting evidence as decided there.

### Acceptance criteria

- [ ] `.github/workflows/ci.yml` remains roughly two jobs (`rust`, `python`);
- [ ] routine CI wall-clock/complexity does not materially increase;
- [ ] manual qualification remains separate;
- [ ] release remains manual;
- [ ] no verification registry/gate framework is introduced.

---

## Required verification after cleanup

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
./scripts/verify.sh full
```

If release workflow code changes, manually dispatch it once after consolidation and require all three existing platform jobs to pass.

If an adversarial/deep script is deleted or merged, run the replacement command/path once to prove coverage did not disappear.

---

## Deletion ledger

Append a closure ledger to this plan. For every deletion or merge, record:

| Artifact | Action | Reason | Replacement/owner | Verification |
|---|---|---|---|---|
| scripts/verify-cargo-packages.sh metadata parsing | corrected | Package verification required an unlisted jq executable even though Cargo metadata is JSON | Python standard-library metadata parsing | bash scripts/verify-cargo-packages.sh --mode all |
| `scripts/bench_compat.py` | deleted | Unreferenced Plan 126 one-off benchmark helper; no current benchmark or CI caller | Current Criterion benches under `crates/eggserve-core/benches/`; historical benchmark records remain | `rg` caller audit; full verification |
| `scripts/__init__.py` | deleted | `scripts/` is not imported as a Python package and the file had no callers | Standalone stdlib scripts | `rg` caller audit; Python checks |
| `docs/architecture.md` | deleted | Duplicated the maintained subsystem map in `architecture/overview.md` | `architecture/overview.md` | `rg` reference audit; docs link check |
| `docs/release-criteria.md` | deleted | Explicitly historical and referenced removed gate-registry tooling | `docs/release-process.md` and manual `verify.sh` tiers | `rg` reference audit; release docs review |
| `.github/workflows/release.yml` | consolidated | Linux, macOS, and Windows jobs repeated the same wheel build, composition, smoke, and upload logic | One explicit three-target matrix; platform-specific flags/paths remain in matrix data | YAML parse/lint; local wheel composition check; GitHub Actions run 31861291909 passed all three targets |
| Wheel composition heredocs in release workflow | extracted | Identical inline Python assertion was repeated on all three targets | `scripts/check-wheel-composition.py` (Python stdlib only) | Script run against local wheel |
| `windows-plan086` feature | renamed | Live manual qualification gate encoded a historical plan number | `windows-adversarial-qualification` | Feature-reference audit; Rust feature builds |
| `scripts/verify.sh` package calls | consolidated | Full verification packaged the core crate twice by invoking `core` and `bin` separately | One `--mode all` package verification | `./scripts/verify.sh full` |
| `crates/eggserve-core/benches/*.rs` and Criterion manifest entries | deleted | Both benchmark harnesses targeted a removed service API and failed all-target compilation; retained benchmark results are historical rather than a current CI gate | Historical `benchmarks/088-baseline/` records; selected future measurements can use a dedicated current harness | `cargo check --workspace --all-targets --all-features`; benchmark-reference audit |
| `tls_service_parity.rs` head-only callbacks | corrected | TLS feature tests still used the pre-`Request` callback signature | Current `service_fn`/`service_fn_head` API | `cargo check --workspace --all-targets --all-features`; TLS tests |

This is a one-time human-readable table, not a generated registry.

---

## Rejection conditions

Reject an implementation that:

- replaces workflow duplication with an opaque custom CI framework;
- moves manual release/security qualification into routine CI;
- deletes adversarial tests because they are expensive;
- deletes historical plans en masse to make the repo look smaller;
- removes public API based only on internal call-site absence;
- introduces new dependencies to simplify scripts;
- changes runtime behavior without a concrete cleanup requirement;
- combines all documentation into one giant file;
- adds release publication automation.

---

## Final acceptance criteria

Plan 130 is complete when:

- [x] deletion inventory is completed;
- [x] stale/unreferenced artifacts are removed;
- [x] release workflow duplication is materially reduced;
- [x] `scripts/` has a small explainable verification hierarchy;
- [x] unused plan-era feature flags/scaffolding are removed;
- [x] unused dev-dependencies are removed without weakening tests;
- [x] current security/conformance/fuzz assets remain available manually;
- [x] routine CI stays small and green;
- [x] manual release workflow passes after consolidation;
- [x] deletion ledger records what changed and why.
