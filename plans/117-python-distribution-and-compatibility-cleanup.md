# Plan 117 — Python Distribution and Compatibility Cleanup

## Status

**COMPLETE.**

See implementation details below.

---

## Goal

Make the Python distribution surface match EggServe's intended product with the least packaging complexity practical.

This phase addresses two narrow questions:

1. Is unconditional TLS compilation in the Python native extension intentional and worth its dependency/artifact cost?
2. Can Python support reasonably be broadened beyond CPython 3.14 without introducing a large release matrix or weakening the native API?

The Python `http.server`-shaped API and installed-wheel behavior are protected. This is not a Python feature-expansion phase.

---

## Product contract to preserve

The supported Python surface remains centered on:

```text
eggserve.server
  HTTPServer
  ThreadingHTTPServer
  HTTPSServer
  ThreadingHTTPSServer
  BaseHTTPRequestHandler
  SimpleHTTPRequestHandler

eggserve.lowlevel
  bounded advanced HTTP/security primitives

eggserve.subprocess
  bundled CLI lifecycle helpers

package root
  serve_directory() where currently supported
```

Do not expose internal native callback types or deleted client functionality as new top-level APIs.

---

## Non-goals

Do not:

- add ASGI/WSGI support;
- create pure-Python fallbacks for the Rust runtime;
- add alternative TLS libraries;
- create a many-version/many-platform GitHub release workflow;
- automate PyPI publication;
- broaden to unsupported Python implementations such as PyPy unless PyO3 support is already trivial and verified;
- redesign the `http.server` facade;
- add HTTP client APIs;
- split the package into multiple separately published distributions merely to make TLS optional;
- reduce compatibility with CPython 3.14 while attempting to broaden support.

---

# Track A — Establish the actual compiled Python surface

Inspect:

```text
crates/eggserve-python/Cargo.toml
crates/eggserve-python/pyproject.toml
crates/eggserve-python/src/lib.rs
crates/eggserve-python/src/server/
crates/eggserve-python/python/eggserve/
crates/eggserve-python/tests/
scripts/test-python-wheel.sh
docs/python-api.md
docs/python-http-server-compatibility.md
architecture/eggserve-python.md
README.md
```

Record:

- modules compiled into `_native`;
- Rust features enabled on `eggserve-core`;
- direct Python-crate dependencies and feature flags;
- which Python classes/functions are re-exported publicly;
- wheel's bundled CLI path;
- current interpreter requirement and wheel tags;
- how HTTPS server classes depend on Rust TLS support.

### Acceptance criteria

- manifest features and compiled module declarations agree;
- no removed Plan 113 client source is still described as compiled;
- HTTPS server behavior has a clearly identified dependency path.

---

# Track B — Decide the Python TLS policy

Current packaging enables EggServe core TLS from the Python extension so that `HTTPSServer` / `ThreadingHTTPSServer` can be available from the same wheel.

Do not remove that functionality merely because the extension is larger.

Evaluate three possibilities:

### Option 1 — Keep TLS unconditional in the wheel

Prefer this if:

- `HTTPSServer` is an intentional part of Python `http.server` parity;
- making TLS optional would require multiple wheel variants or import-time feature fragmentation;
- dependency/artifact cost is modest;
- one wheel with predictable API is simpler for users.

If retained, document explicitly that:

```text
standalone Rust CLI: TLS feature is optional at build time
Python wheel: native extension includes TLS so HTTPSServer is consistently available
```

This asymmetry is acceptable if intentional.

### Option 2 — Remove duplicate direct TLS ownership but keep feature activation

Prefer this where the Python crate directly declares TLS-related dependencies that it does not import itself and only needs `eggserve-core/tls` to activate the implementation.

Plan 114 should already have removed obvious redundancy. Confirm no direct declaration remains without a source-level reason.

### Option 3 — Make Python TLS optional

Only choose this if it can be achieved with one simple package contract and no confusing wheel variants. If making TLS optional means separate distributions, custom environment-selected builds, or a larger publication matrix, reject this option as over-engineering.

### Default decision

Unless measurement shows a disproportionate cost or the current code reveals a much simpler mechanism, keep HTTPS classes consistently available in the Python wheel and optimize only redundant dependency ownership.

### Acceptance criteria

- TLS policy is explicit rather than accidental;
- Python HTTPS classes either remain consistently available or any intentional change is clearly justified;
- no multiple-wheel feature matrix is introduced;
- standalone Rust TLS remains optional.

---

# Track C — Evaluate abi3 and Python-version broadening

The current `pyproject.toml` limits installation to:

```text
>=3.14,<3.15
```

That is unnecessarily narrow for a standard-library-shaped package unless required by a concrete implementation dependency.

Investigate the currently used PyO3 version and APIs for `abi3` compatibility. Prefer the lowest-complexity supported strategy.

Questions to answer:

1. Does the extension use CPython APIs unavailable through PyO3 stable ABI support?
2. Can `pyo3` be configured with an `abi3-pyXY` feature while retaining all current functionality?
3. What minimum Python version is reasonable for the project based on dependency support and desired maintenance burden?
4. Can one abi3 wheel per OS/architecture serve multiple CPython minor versions?
5. Does the bundled CLI packaging remain independent of interpreter minor version?
6. Does maturin configuration need only a small declarative change, or would broadening require custom build scripting?

### Preferred outcome

If straightforward, adopt abi3 with a conservative minimum Python version supported by current PyO3 and the package code, and set `requires-python` accordingly.

Do not select an unnecessarily old minimum version merely to maximize a number. A modest supported range with low maintenance is better than broad nominal compatibility.

### Conditional stop rule

If abi3 requires invasive native API rewrites, material functionality loss, or a complex wheel build matrix, do not implement it in this phase. Instead:

- retain the current interpreter restriction temporarily;
- document the concrete blocker;
- do not create another plan solely to chase Python-version breadth.

### Acceptance criteria

Either:

A. Python support broadens with a simple, tested abi3-compatible configuration; or

B. the current 3.14-only restriction remains with a precise technical justification rather than inertia.

---

# Track D — Keep wheel build/install verification authoritative

Any Python packaging change must be verified through an installed wheel, not only `PYTHONPATH` source imports.

The test path must continue to prove:

- wheel builds;
- bundled CLI is included;
- package imports from an isolated environment;
- `eggserve.server` facade works through real sockets;
- HTTPS facade works if it remains in the supported wheel contract;
- native extension loads on the target interpreter;
- Python callback/request/response behavior remains bounded and fail-closed.

If abi3 is adopted, test at least:

- the minimum supported CPython version available in the verification environment;
- the newest supported/current development target used by the project.

Do not create a large CI matrix. A small compatibility check may be manual/release-time if routine CI cost would be disproportionate.

### Acceptance criteria

- installed-wheel verification remains the canonical Python distribution test;
- no source-tree-only success is accepted as packaging proof;
- broader compatibility, if claimed, is tested on more than one relevant interpreter version before release.

---

# Track E — Packaging metadata and classifiers

If Python support changes, update:

```text
requires-python
classifiers
maturin/PyO3 feature configuration
Python API docs
README installation text
release-process docs if necessary
```

Do not add classifiers for versions not actually tested/supported.

If abi3 changes the wheel tag, document that as a packaging fact, not a new product feature.

Keep platform support claims aligned with actual wheel build practices. Do not imply routine CI builds Windows/macOS wheels if releases remain manual.

### Acceptance criteria

- metadata matches tested support;
- classifiers are truthful;
- manual release policy is unchanged;
- wheel-installed CLI behavior is unchanged.

---

# Track F — Artifact measurement

Repeat Python measurements after packaging changes:

- `_native` member size;
- bundled CLI member size;
- compressed wheel size.

Compare against the post-114 baseline, not only old Plan 109 values.

If keeping TLS unconditional, record its approximate cost only if a comparable build without TLS can be produced locally without changing the published package contract. Do not create permanent alternate artifacts solely for measurement.

### Acceptance criteria

- measurement does not become a gate;
- no unsupported size claim is made;
- package simplification is not rejected for tiny compiler-noise regressions.

---

# Track G — Verification

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
bash scripts/test-python-wheel.sh
```

Additionally run Python compatibility builds/tests for each interpreter version newly claimed.

If TLS dependency ownership changes, run the HTTPS compatibility suite explicitly.

Search for stale version/TLS claims:

```sh
rg -n "3\.14|CPython 3\.14|abi3|HTTPSServer|ThreadingHTTPSServer|tls" \
  crates/eggserve-python README.md docs architecture AGENTS.md
```

Classify historical-plan references separately from active docs.

---

## Final acceptance criteria

Plan 117 is complete when:

- the Python wheel's TLS policy is explicit and low-complexity;
- redundant direct TLS/dependency ownership is removed where safe;
- `HTTPSServer` behavior remains available if retained in the supported contract;
- no wheel feature matrix or automated publication system is introduced;
- Python-version support is broadened through a simple abi3 strategy if feasible, otherwise the narrow range is technically justified;
- package metadata matches actual tested support;
- installed-wheel tests pass;
- default standalone Rust TLS remains optional;
- no new Python product surface is added.

## Rejection conditions

Reject the implementation if it:

- removes HTTPS facade functionality solely to reduce wheel size;
- creates separate TLS/non-TLS PyPI packages;
- creates a large CI/release matrix;
- claims abi3 compatibility without testing multiple relevant interpreter versions;
- rewrites the Python facade around stable-ABI limitations if the complexity outweighs the compatibility benefit;
- adds pure-Python networking fallbacks;
- exposes deleted client functionality as a new Python API.
