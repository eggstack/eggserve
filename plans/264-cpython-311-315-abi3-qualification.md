# Plan 264 — CPython 3.11–3.15 stable-ABI qualification

## Purpose

Extend the existing `cp311-abi3` Python support contract through CPython 3.15
without producing redundant per-minor wheels, and make the release/CI evidence
prove the claim rather than relying only on the ABI tag.

Depends on Plan 263. No platform breadth is added here.

Planning baseline:

```text
09ada539 docs: simplify readme around python/rust quick starts
```

## Current state

- PyO3 is pinned to 0.29.2 with `abi3-py311`.
- `requires-python = ">=3.11"`.
- classifiers currently stop at Python 3.14.
- routine Python CI builds/tests with CPython 3.14.
- release builds use Python 3.11 as the interpreter baseline and prove the
  representative Linux x86_64 wheel on CPython 3.14.
- `scripts/check-python-release-metadata.py` verifies `abi3-py311` but does
  not prove the full supported minor range.
- `scripts/test-python-wheel.sh` defaults to `python3.14`.

The objective is to prove that **the same wheel bytes** work on 3.11, 3.12,
3.13, 3.14, and 3.15.

## Track A — Metadata and support contract

Files:
- `crates/eggserve-python/pyproject.toml`
- `docs/toolchain-support.md`
- `docs/release-contract.md`
- `docs/release-process.md`
- `SECURITY.md`
- `README.md` only if the concise install claim needs clarification.

Changes:

1. Add the Python 3.15 classifier.
2. Keep `requires-python = ">=3.11"`; do not add an artificial upper bound.
3. State explicitly that GIL-enabled CPython 3.11–3.15 is qualified through one
   `cp311-abi3` artifact per platform.
4. Keep PyPy unsupported.
5. Keep free-threaded CPython outside this plan; Plan 267 owns that decision.
6. Remove stale "newest supported = 3.14" wording from release docs.

Do not change the PyO3 ABI feature unless qualification exposes a real blocker.

## Track B — Build-once, test-many ABI proof

Refactor the representative ABI proof so it does not accidentally prove five
separately built wheels.

Preferred structure:

1. Build one Linux x86_64 `cp311-abi3` wheel using the release baseline
   (`--interpreter python3.11`).
2. Upload that exact wheel as an artifact.
3. Fan out a test matrix over:
   - CPython 3.11
   - CPython 3.12
   - CPython 3.13
   - CPython 3.14
   - CPython 3.15
4. Each matrix entry downloads the same artifact, installs it with
   `--only-binary=:all:`, and runs:
   - `import eggserve`
   - `import eggserve._native`
   - `eggserve --help`
   - `python -m eggserve --help`
   - `scripts/release_smoke.py`
   - a compact stable-ABI-facing fixture exercising representative native
     classes/functions, not only import.
5. Record the installed wheel filename and interpreter version.

If CPython 3.15 final is not available in the selected GitHub runner/toolcache
at execution time, use the latest available 3.15 release candidate with the
setup action's prerelease mechanism. Once final 3.15 is available, remove the
prerelease exception rather than carrying it indefinitely.

Do not use `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` as a substitute for runtime
qualification. It may remain where needed to build against a newer interpreter,
but the evidence is the installed released wheel.

## Track C — Reusable local harness

Make `scripts/test-python-wheel.sh` less coupled to 3.14:

- keep `PYTHON=` override;
- derive its default from an explicitly documented project test version rather
  than embedding support semantics in the shell script;
- allow testing an already-built wheel path so the same wheel can be reused
  across interpreter lanes;
- preserve the full installed-wheel suite for the primary CI interpreter;
- provide a lighter ABI-smoke mode for every supported minor to control CI
  cost.

Do not duplicate the Python test suite five times unless a failing ABI smoke
demonstrates version-sensitive behavior requiring it.

## Track D — Release metadata gates

Extend `scripts/check-python-release-metadata.py` to fail cheaply when:

- the PyO3 dependency no longer contains `abi3-py311`;
- `requires-python` no longer includes the 3.11 floor;
- the supported-minor metadata omits 3.15 after this plan lands;
- documentation/config claims a different normal ABI baseline than the
  executable packaging configuration.

Keep the check semantic. Do not hard-code incidental workflow line formatting.

## Track E — Release and post-publish proof

Update the release workflow so the aggregate wheel set remains one wheel per
platform, while the representative published-package smoke includes both ends
of the supported Python range where practical:

- minimum: CPython 3.11;
- maximum: CPython 3.15.

The post-publish check must use binary-only resolution and fail if pip falls
back to an sdist/local build.

## Verification

Required local/static checks:

```sh
python3 scripts/check-python-release-metadata.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo fmt --all -- --check
```

Required CI evidence:

- one `cp311-abi3` Linux x86_64 wheel artifact SHA/filename;
- that exact artifact installed successfully on 3.11, 3.12, 3.13, 3.14, 3.15;
- representative native smoke passes on each;
- full installed-wheel Python suite still passes on the primary CI interpreter;
- no per-minor `cp312-cp312`/.../`cp315-cp315` wheels appear in the release
  artifact set.

## Acceptance

- `pyproject.toml` advertises Python 3.15.
- normal CPython support is documented as 3.11–3.15 using `cp311-abi3`.
- one wheel is proven across all five minors.
- release scripts/validators continue to require `cp311-abi3`.
- no Python or Rust public API change.
- free-threaded CPython remains explicitly outside the supported contract until
  Plan 267 reaches a separate evidence-backed decision.
