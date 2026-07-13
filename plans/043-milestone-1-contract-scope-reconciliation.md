# Phase 43 — Milestone 1A: Contract, Scope, and Support Reconciliation

## Goal

Establish one coherent, reviewable product contract before expanding eggserve's public library APIs. This phase is documentation- and contract-heavy, but it is not cosmetic: later API and release decisions must have one authoritative answer for scope, supported platforms, supported language versions, TLS status, stability guarantees, and downstream adapter expectations.

The implementation must reconcile existing repository claims without broadening product scope. It must not add new HTTP features, runtime capabilities, or framework behavior.

## Why this phase is required

The repository has accumulated several independently accurate but partially inconsistent documents. Examples include:

- the README presenting implemented TLS options while `docs/non-goals.md` still describes TLS as deferred;
- different documents emphasizing the CLI, primitive library, Python server callbacks, or experimental client without one capability matrix;
- stable API language that needs a precise pre-1.0 compatibility interpretation;
- platform support claims that must distinguish functional support from hardened filesystem confinement;
- Python packaging metadata that must remain aligned with the actually tested CPython range;
- release and security documents that must agree on accepted limitations.

Expanding the library before resolving these statements risks stabilizing APIs against an ambiguous product definition.

## Scope

Review and reconcile at least:

- `README.md`;
- `docs/non-goals.md`;
- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `docs/security-policy.md`;
- `docs/python-api.md`;
- `docs/python-packaging.md`;
- `docs/release-criteria.md`;
- `docs/release-checklist.md`;
- `docs/dependency-policy.md`;
- architecture documents for server, Python, client, filesystem, and TLS paths;
- root and crate `Cargo.toml` metadata;
- Python `pyproject.toml`;
- `SECURITY.md`;
- CLI help text and startup warnings;
- crate-level Rust documentation and Python package exports;
- examples and installation instructions.

## Track A — Build an authoritative capability inventory

Create `docs/library-capability-matrix.md` or an equivalently named document.

The matrix must identify each capability and its status across:

- CLI;
- Rust stable API;
- Rust experimental API;
- Python stable API;
- Python experimental API;
- built-in static service;
- generic callback server;
- experimental client.

Capabilities should include at least:

- bind/listen lifecycle;
- plaintext HTTP/1.x;
- TLS server;
- TLS client;
- GET/HEAD static serving;
- request-target validation;
- request-body rejection;
- bounded request-body support;
- secure root resolution;
- symlink policy;
- dotfile policy;
- directory listing;
- index files;
- conditional requests;
- ranges;
- file streaming;
- generic byte responses;
- duplicate headers;
- callback handlers;
- existing-listener support;
- graceful shutdown;
- observability hooks;
- redirects, retries, cookies, proxies, decompression;
- ASGI/WSGI adapters;
- Windows reparse-point hardening.

For each entry use a constrained vocabulary such as:

- stable;
- experimental;
- internal;
- CLI-only;
- planned;
- intentionally unsupported;
- platform-limited.

Do not use vague terms such as "mostly supported" without a note defining the limitation.

## Track B — Reconcile product identity and non-goals

Update the product description so all primary documents agree that eggserve is:

- a hardened static server;
- a library of safe HTTP/filesystem primitives;
- a reusable Rust-owned server runtime with Python projections;
- a substrate for downstream adapters.

Make clear that downstream projects may build ASGI, WSGI, routing, middleware, authentication, and application semantics on top of eggserve, but these are not implemented in-tree.

Correct stale non-goals language. In particular:

- TLS should no longer be described as deferred if server TLS is implemented and tested;
- automatic ACME remains out of scope;
- the callback server must be acknowledged without describing eggserve as a full application framework;
- the experimental client must be acknowledged without implying requests/httpx parity;
- Windows functional support must remain distinct from hardened untrusted-content support.

Ensure the README's concise scope summary and `docs/non-goals.md` agree exactly on the boundary.

## Track C — Define stability and compatibility policy

Clarify the practical meaning of each stability tier.

### Stable

Specify:

- stable names and signatures are intentionally supported;
- patch releases must not break stable APIs;
- while `<1.0`, a minor release may break stable APIs only with explicit release notes and migration guidance;
- semantic behavior identified in the release contract is covered by conformance tests;
- unspecified formatting, debug output, log text, and internal implementation details are not stable unless explicitly documented.

### Experimental

Specify:

- may change in any non-patch or, if existing policy requires, any release;
- consumers should pin versions;
- functionality is tested but the interface is not frozen;
- experimental APIs may be omitted from language parity.

### Internal

Specify:

- unavailable or unsupported for downstream use;
- no compatibility guarantee;
- internal Python names are not exported through `__all__`;
- internal Rust features do not become accidental default features.

Decide and document:

- whether stable enum variants are exhaustive;
- whether exception classes, fields, and messages are stable;
- whether header ordering and duplicate preservation are stable;
- whether denial/error taxonomy variants are stable;
- whether serialization or repr output is stable;
- deprecation requirements before removal.

Update `docs/api-stability.md` and `docs/release-contract.md` accordingly.

## Track D — Reconcile language and toolchain support

Define and document:

- MSRV or an explicit policy that current stable Rust is required;
- whether MSRV is tested in CI;
- supported CPython versions;
- unsupported Python implementations such as PyPy or free-threaded CPython;
- supported wheel architectures;
- supported operating systems;
- ABI strategy if abi3 is used later;
- minimum maturin and PyO3 constraints.

The following must agree:

- `pyproject.toml` `requires-python`;
- classifiers;
- README installation section;
- Python packaging docs;
- CI matrix;
- release criteria;
- generated release checklist.

Do not claim a language version merely because the source code appears compatible. A supported version must have an explicit execution gate.

## Track E — Reconcile platform security classifications

Create one platform classification table used or referenced everywhere.

Recommended categories:

- `supported-hardened`;
- `supported-functional`;
- `release-target`;
- `build-only`;
- `unsupported`.

At minimum classify:

- Linux x86_64;
- Linux aarch64;
- macOS arm64;
- macOS x86_64;
- Windows x86_64.

For Windows, preserve the limitation that parser-level and functional support do not equal Unix-style handle/descriptor-relative reparse-point hardening.

For follow-symlinks mode, preserve the limitation that it is weaker than default no-follow descriptor-relative traversal.

Ensure the same limitation appears in:

- README;
- security policy;
- release contract;
- release criteria;
- release checklist;
- PyPI-facing long description where practical;
- CLI startup warning for public exposure on Windows if that warning already exists or is added without changing server behavior.

## Track F — Reconcile TLS claims

Inventory the actual TLS implementation and tests before editing claims.

Document separately:

- server TLS feature;
- client TLS feature;
- certificate and key format;
- verification defaults;
- handshake timeout behavior;
- unsupported certificate management features;
- no automatic ACME;
- whether TLS is compiled into published binaries/wheels;
- exact validation jobs required for release.

Do not claim production TLS support based only on compilation. Claims must map to tests in the release criteria manifest created in Phase 44.

## Track G — Reconcile crate/package metadata

Review:

- package descriptions;
- repository/homepage/documentation URLs;
- license declarations;
- keywords/categories;
- included files;
- feature descriptions;
- version consistency;
- README paths;
- Python package metadata;
- CLI binary inclusion in wheels.

Ensure language such as "static server," "primitive library," "server runtime," and "experimental client" is used consistently.

Do not advertise unsupported framework or protocol capabilities for search visibility.

## Track H — Add contract consistency tests

Add tests or scripts that fail when important claims drift.

At minimum validate:

- Python version metadata matches the declared support document;
- package versions remain synchronized where required;
- platform classifiers correspond to criteria entries;
- stable Python exports correspond to the API inventory;
- Rust stable exports are represented in the inventory or an allowlisted mechanism;
- TLS feature claims correspond to feature definitions and validation gates;
- README links resolve to repository files;
- no document still describes an implemented feature as deferred;
- generated capability matrix markers use only the allowed vocabulary.

A small script under `scripts/` is acceptable. It should emit actionable errors naming the conflicting files and values.

## Required deliverables

- authoritative capability matrix;
- reconciled README and non-goals;
- reconciled release contract and API stability policy;
- language/toolchain support policy;
- platform support/security table;
- TLS support statement;
- metadata corrections;
- contract consistency validator;
- tests for the validator;
- updates to AGENTS/contributor guidance explaining which documents are authoritative.

## Required validation

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --doc
python -m unittest discover -s crates/eggserve-python/python -p 'test*.py' -v
bash scripts/verify-cargo-packages.sh
```

Also run the new contract consistency command directly and through CI.

If Python package tests require a built extension, preserve existing separation between source-only unit tests and installed-wheel tests. Do not weaken existing clean-wheel rules.

## Review checklist

Reviewers should verify:

- scope was reconciled rather than broadened;
- no new framework commitment was introduced;
- TLS claims match actual implementation and tests;
- Windows and follow-symlink limitations remain explicit;
- every supported language/platform claim has a future gate in the criteria model;
- pre-1.0 compatibility language is unambiguous;
- experimental client status remains clear;
- the capability matrix distinguishes absence from intentional non-goal.

## Completion criteria

This phase is complete only when:

- all public-facing documents tell the same product story;
- stale TLS and scope statements are removed;
- one capability matrix describes Rust/Python/CLI parity;
- stability tiers have operational compatibility rules;
- language and platform support claims are exact;
- Windows and symlink limitations are consistently prominent;
- metadata agrees with documentation;
- automated consistency checks fail on representative drift;
- no public API expansion was performed as part of this phase.

## Non-goals

- no canonical request/response redesign;
- no request-body support;
- no server runtime refactor;
- no ASGI/WSGI adapter;
- no routing, middleware, auth, proxy, ACME, or WebSocket work;
- no attempt to complete Windows reparse-point hardening;
- no final release publication.