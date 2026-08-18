# Plan 139 — Python Release Version and Wheel Contract

## Status

**READY FOR HANDOFF — 2026-08-18.**

Parent roadmap: Plan 138.

Reviewed repository baseline:

```text
main = 2b90abe0b118c03ea23cc37d63a4fe35b174bfdd
```

This plan establishes one trustworthy Python release identity and one explicit ABI/wheel contract before the release workflow is widened or granted PyPI publication capability.

---

# 1. Why this plan exists

The existing Python packaging architecture is suitable for wide binary distribution, but its release metadata is currently inconsistent.

At the reviewed baseline:

```text
Cargo workspace package version                       = 0.1.2
crates/eggserve-python/Cargo.toml package version     = 0.1.0
crates/eggserve-python/pyproject.toml project version = 0.1.0
python/eggserve/__init__.py __version__                = 0.1.0
```

The Python crate is excluded from the root Cargo workspace and is built independently by Maturin, so this drift is easy to create accidentally.

At the same time, release builds currently use CPython 3.14 plus:

```text
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
```

because the repository's pinned PyO3 0.24 line predates official CPython 3.14 support. The extension itself is configured with `abi3-py311`, so release artifact production does not need to depend on Python 3.14 merely to produce the stable-ABI wheel.

Before adding nine or more platform artifacts, the release identity and ABI contract must be deterministic. Otherwise a wider matrix only multiplies inconsistent artifacts.

---

# 2. Goals

This phase must:

1. establish one intentional EggServe release version across Rust and Python packaging surfaces;
2. remove avoidable duplicate Python runtime version ownership;
3. add a small release preflight that fails on metadata drift;
4. make `cp311-abi3` the explicit Python wheel contract;
5. build release wheels using the minimum supported CPython ABI baseline rather than requiring forward-compatibility mode against a newer interpreter;
6. retain compatibility proof on both the minimum and newest supported CPython versions;
7. preserve the existing Python package, API, native extension, and extension-backed CLI architecture.

---

# 3. Non-goals and hard boundaries

Do not:

- add a release-management framework;
- adopt setuptools, scikit-build, cibuildwheel, or another packaging stack on top of Maturin without a demonstrated requirement;
- move the Python crate into the Rust workspace solely to share a version;
- require a PyO3 upgrade merely to eliminate the release-time forward-compatibility environment variable;
- create Python 3.11/3.12/3.13/3.14 wheel variants;
- support PyPy or free-threaded CPython in this phase;
- alter the Python API surface;
- change the extension module name;
- reintroduce a bundled standalone executable;
- add release automation triggered by tags or pushes;
- couple Python publication to crates.io publication.

Keep this phase limited to release metadata, stable-ABI artifact identity, and preflight validation.

---

# 4. Track A — Define the authoritative release version relationship

## Required relationship

An EggServe source release should not encode contradictory product versions in the Rust workspace and Python distribution.

For a release commit, require:

```text
workspace.package.version
    == crates/eggserve-python/package.version
    == Python distribution metadata version
    == importlib.metadata.version("eggserve") after installation
```

The implementation may keep the Python crate's Cargo version explicit because the crate is intentionally excluded from the root workspace. The important requirement is deterministic validation, not clever inheritance.

## Preferred simplification

Eliminate the separately hard-coded runtime value in:

```text
crates/eggserve-python/python/eggserve/__init__.py
```

Prefer deriving `eggserve.__version__` from installed distribution metadata:

```python
from importlib.metadata import version

__version__ = version("eggserve")
```

If source-tree development genuinely requires a fallback, keep that fallback narrow and clearly non-authoritative. Do not maintain another literal release number as the fallback.

The wheel's installed metadata should remain authoritative for Python users.

## `pyproject.toml` version ownership

Investigate the smallest safe Maturin-supported approach for avoiding unnecessary duplication between the Python crate's Cargo package version and `pyproject.toml`.

Preferred ordering:

1. if current Maturin can reliably derive the Python distribution version from Cargo metadata for this mixed Python/Rust layout, use that documented mechanism;
2. otherwise keep both values explicit and enforce equality with the preflight script.

Do not introduce dynamic-version plugins or a version file generator merely to remove one literal.

### Acceptance criteria

- a release commit cannot pass preflight with Rust workspace 0.1.2 and Python distribution 0.1.0;
- installed `eggserve.__version__` agrees with installed distribution metadata;
- the solution is understandable from the manifests without a release framework;
- source development remains usable.

---

# 5. Track B — Add a standard-library release metadata preflight

Add one small script, preferably:

```text
scripts/check-python-release-metadata.py
```

or an equivalently narrow name.

It should use Python's standard library only.

## Required checks

At minimum validate:

### B1. Package identity

```text
pyproject project name == eggserve
Maturin module-name == eggserve._native
project script eggserve == eggserve._bin:main
```

### B2. Release version

Check the authoritative version surfaces selected in Track A and reject any mismatch.

When run from the release workflow, optionally accept an expected version argument only if that prevents accidental publication of the wrong commit. Do not make a manually typed version the new source of truth.

### B3. Python compatibility contract

Validate:

```text
requires-python includes >=3.11
PyO3 feature includes abi3-py311
Maturin bindings remain pyo3
```

Do not attempt to implement a general PEP 440 parser if the existing metadata can be checked directly and conservatively.

### B4. Wheel architecture contract

The preflight should assert the source configuration still describes one native extension wheel, not a separately staged executable.

Reuse `scripts/check-wheel-composition.py` for built-artifact composition; do not duplicate ZIP member logic unnecessarily.

### B5. Release tooling versions

Keep Maturin pinned to the project-selected release version in workflow configuration. If the repository uses a Maturin action, the action and Maturin CLI version must both be explicit rather than floating.

### Acceptance criteria

- the script exits nonzero on any release-version mismatch;
- the script exits nonzero if `abi3-py311` is removed accidentally;
- routine execution requires no network access and no third-party Python module;
- release workflow runs the preflight before any expensive platform builds.

---

# 6. Track C — Build release wheels against the minimum ABI baseline

## Problem with the current release build

The current release workflow installs Python 3.14 and sets:

```text
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
```

This is acceptable as a compatibility test workaround for the pinned PyO3 version, but it should not be required to create the release artifact when the wheel contract is `abi3-py311`.

## Required release-build behavior

Build the canonical ABI3 wheel using CPython 3.11 or the documented Maturin/PyO3 equivalent for an `abi3-py311` extension.

The desired artifact identity is conceptually:

```text
cp311-abi3-<platform>
```

not:

```text
cp314-cp314-<platform>
```

and not one wheel per Python minor version.

If Maturin can construct the abi3 wheel without an explicitly installed build interpreter for a given cross target, use the simpler supported method. The resulting wheel tag and runtime compatibility are the acceptance criteria.

## Forward-compatibility environment variable

Remove `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` from **release artifact construction** if it is no longer needed after using the minimum supported interpreter.

It may remain in an existing CPython 3.14 routine test job while the project stays on PyO3 0.24, because that job answers a different question: whether the existing stable-ABI extension remains usable with the project's newest supported interpreter.

Do not conflate:

```text
build baseline
```

with:

```text
newest-interpreter compatibility test
```

### Acceptance criteria

- release wheel construction does not rely on CPython 3.14 forward-compatibility mode;
- built wheel is tagged for the intended stable ABI;
- no per-interpreter artifact multiplication is introduced;
- existing CPython 3.14 compatibility remains tested somewhere appropriate.

---

# 7. Track D — Prove both ends of the declared Python range

The package declares CPython 3.11+ support. ABI3 reduces build multiplicity but does not remove the need to test the claim.

For at least one representative native release platform, preferably glibc Linux x86_64:

1. build the release wheel once against the 3.11 ABI baseline;
2. install that **same wheel file** under CPython 3.11;
3. run import + CLI + real server smoke;
4. install that same wheel file under CPython 3.14;
5. run the same smoke;
6. where inexpensive, run the existing installed-wheel Python test suite under both ends.

Do not rebuild between the two interpreter tests. The purpose is to prove stable-ABI reuse of one artifact.

If Python 3.15 becomes a supported stable interpreter before implementation is complete, update the newest-interpreter side of this check deliberately rather than silently extending classifiers.

### Acceptance criteria

- one built wheel is proven on minimum and newest supported CPython;
- package metadata claims no untested Python minor version beyond the project's explicit support policy;
- source-tree imports cannot satisfy the test accidentally.

---

# 8. Track E — Wheel metadata verification after build

Extend the existing artifact checks with a small wheel metadata inspection step.

For every release wheel, validate:

```text
Name: eggserve
Version: <expected release version>
Requires-Python: >=3.11 or the exact retained compatible expression
Root-Is-Purelib: false
wheel tag contains cp311-abi3
```

The exact `WHEEL`/`METADATA` fields should be read from the archive using `zipfile` and standard-library email/config parsing where useful.

Do not add `wheel`, `packaging`, or another Python dependency solely to inspect these fields unless current standard-library parsing is demonstrably insufficient.

Combine or extend `check-wheel-composition.py` if that produces a clearer single artifact validator. Avoid proliferating several tiny scripts that partially overlap.

### Acceptance criteria

- a wheel with the wrong release version is rejected before upload;
- a wheel that unexpectedly becomes interpreter-specific is rejected;
- wheel composition still proves there is no second standalone `eggserve` executable;
- checks run against the final artifact that will be aggregated for publication.

---

# 9. Track F — Release workflow preflight ordering

Modify the manual release workflow so inexpensive source-level correctness fails before the platform matrix starts.

Recommended order:

```text
preflight job
  checkout exact selected ref
  record commit SHA
  run check-python-release-metadata.py
  verify Cargo.lock / required manifests exist
  expose expected package version as job output

build jobs
  depend on preflight
  consume expected version
  build artifact
  inspect artifact metadata
  smoke artifact
  upload artifact
```

The expected version should flow from the repository metadata, not from a free-form matrix string copied into every job.

### Acceptance criteria

- a metadata mismatch prevents all platform builds;
- every build uses the exact same source commit;
- every built artifact is checked against the preflight version;
- no publication credential is present in preflight or build jobs.

---

# 10. Track G — Documentation correction

Update the active packaging/release documentation after the implementation lands.

At minimum explain:

```text
Python minimum: CPython 3.11
ABI: abi3-py311
one wheel per OS/architecture/platform ABI family
release build baseline uses minimum compatible ABI
newer CPython versions consume the same wheel
```

Document any continued use of `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` in routine CPython 3.14 tests as a test-tooling constraint, not a property of the released wheel.

Remove stale wording implying that wheel publication requires Python 3.14 to build.

### Acceptance criteria

- developer docs describe the same ABI/version model enforced by scripts;
- no documentation says the wheel is 3.14-specific;
- the relationship between Rust workspace version and Python distribution version is explicit enough for a maintainer to perform the next release correctly.

---

# 11. Required verification

Before this plan is complete, run at least:

```text
python3 scripts/check-python-release-metadata.py
cargo check/test paths already relevant to manifest changes, if any
Maturin build of a Linux x86_64 release wheel using the new ABI baseline
wheel artifact metadata/composition validation
installed-wheel smoke under CPython 3.11
installed same-wheel smoke under CPython 3.14
existing routine Python CI
```

If implementation touches only packaging/version metadata and scripts, do not run unrelated deep fuzzing, Windows adversarial filesystem qualification, soak testing, or other product-level suites.

---

# 12. Acceptance criteria

Plan 139 is complete when:

- the root Rust release version and Python distribution release version cannot drift unnoticed;
- the reviewed current 0.1.2/0.1.0 mismatch is resolved intentionally;
- `eggserve.__version__` derives from installed distribution metadata or otherwise cannot drift independently;
- a standard-library release preflight exists and runs before release matrix builds;
- `abi3-py311` is explicitly validated;
- release artifact construction uses the minimum stable-ABI baseline rather than requiring CPython 3.14 forward compatibility;
- the same built wheel is proven under CPython 3.11 and the newest currently supported interpreter;
- wheel metadata and composition are checked before artifact upload;
- the extension-backed CLI architecture remains unchanged;
- no Python-version matrix, packaging framework, or release-management framework is added;
- routine CI remains small;
- documentation matches the implemented version/ABI model.

Once these criteria pass, proceed to Plan 140. Do not grant PyPI publication capability before this phase is closed.
