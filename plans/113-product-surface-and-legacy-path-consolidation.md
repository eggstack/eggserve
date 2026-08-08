# Plan 113 — Product Surface and Legacy Path Consolidation

## Status

**PLANNED.**

Depends on Plan 112.

This phase removes product and compatibility surfaces that no longer belong in EggServe's supported contract. It is intentionally deletion-biased. The objective is one clear serving architecture and one coherent product story before dependency, packaging, and documentation cleanup proceed.

---

## Goal

After this phase, the repository should implement exactly the product described by Plan 112:

```text
hardened static server
+ reusable HTTP/security primitives
+ Python http.server-shaped facade
```

The repository should not also carry a second, feature-gated HTTP client product or a deprecated pre-runtime serving architecture unless concrete supported consumers prove those surfaces are still required.

---

## Scope

Primary areas to inspect:

```text
crates/eggserve-core/Cargo.toml
crates/eggserve-core/src/lib.rs
crates/eggserve-core/src/primitives/mod.rs
crates/eggserve-core/src/primitives/client/
crates/eggserve-core/src/service.rs
crates/eggserve-core/src/server/
crates/eggserve-core/tests/client_*.rs
crates/eggserve-core/tests/public_api_consumers.rs
crates/eggserve-core/tests/api_stability.rs
crates/eggserve-bin/src/lib.rs
crates/eggserve-python/Cargo.toml
crates/eggserve-python/src/lib.rs
crates/eggserve-python/src/client.rs
crates/eggserve-python/python/eggserve/
crates/eggserve-python/tests/
architecture/client.md
docs/http-client-primitives.md
docs/api-stability.md
docs/release-contract.md
README.md
AGENTS.md
```

Search rather than assuming exact paths remain unchanged.

---

## Non-goals

Do not:

- add new HTTP client functionality;
- migrate client functionality to another repository as part of this plan;
- add redirects, pooling, cookies, decompression, proxies, retries, streaming client bodies, or other client features;
- redesign canonical HTTP primitives that are useful to server and downstream consumers;
- alter filesystem confinement behavior;
- alter static response semantics;
- alter Python `http.server` behavior except where it directly depends on a removed legacy path;
- add compatibility shims for code that was never part of a supported release surface;
- preserve dead alpha-only code solely to avoid a breaking change before 1.0.

---

# Track A — Establish the supported-surface evidence

Before deleting anything, classify every candidate surface.

Use repository evidence from:

- README supported APIs;
- `docs/python-api.md`;
- `docs/release-contract.md`;
- API stability tests;
- package exports;
- crate public re-exports;
- examples;
- current Python module registration;
- published feature documentation;
- release notes if present.

For the HTTP client subsystem, answer explicitly:

1. Is `eggserve_core::primitives::client` part of the documented supported Rust contract?
2. Is a Python client actually compiled and exported from the current extension?
3. Does `pyproject.toml` or package `__init__.py` expose a client-facing API?
4. Does any non-test production code require the `client` or `client-tls` features?
5. Are the client features needed only by internal tests or historical plans?
6. Would removing the client leave the generic canonical HTTP types sufficient for downstream projects to implement clients independently?

For the legacy service adapter, answer:

1. Is `eggserve_core::service` reachable from documented current examples?
2. Is it used by production `eggserve-bin` or Python serving paths?
3. Is it used only by test helpers or compatibility tests?
4. Does any supported API require the deprecated adapter rather than `server::StaticService` / `server::Service`?

### Decision rule

Default to removal when the surface is:

```text
experimental or deprecated
+ not used by production paths
+ not exported by the supported Python facade
+ not required by a documented stable contract
```

If a concrete current supported contract contradicts removal, do not silently break it. Record that evidence in the implementation commit/plan closure and retain only the minimum required compatibility surface.

### Acceptance criteria

- each candidate surface is classified before deletion;
- no deletion decision is based only on source-file existence;
- stable canonical HTTP primitives are not mistaken for the full client product;
- the resulting scope decision is explicit in the implementation record.

---

# Track B — Remove the full HTTP client product if unsupported

If Track A confirms the current client is not an intentional supported release contract, remove it completely rather than leaving a half-maintained feature.

Expected deletions/edits may include:

```text
crates/eggserve-core/src/primitives/client/
client and client-tls feature declarations
client-only optional dependencies such as webpki-roots
ClientError variants/conversions that have no remaining caller
client-only tests
client-only fuzz target(s), including URL parser fuzzing if no non-client owner remains
architecture/client.md
docs/http-client-primitives.md
client references in API stability docs
Python client binding source and references
AGENTS.md client-specific quirks
```

Do not delete a dependency merely because it appears in this list; confirm whether TLS server support or another subsystem also uses it.

### Preserve generic primitives

The following categories remain in scope even if client implementation is removed:

- `Method`;
- `HttpVersion`;
- `HeaderName` / `HeaderValue` / `HeaderBlock`;
- `RequestTarget` and request-head types;
- canonical response types;
- request/response validation helpers where they are server-neutral;
- body abstractions used by server/custom service boundaries;
- connection metadata;
- response normalization.

The criterion is whether the primitive describes HTTP itself, not whether a client once consumed it.

### Python orphan cleanup

Current repository evidence must be reconciled: if `crates/eggserve-python/src/client.rs` exists but is not declared as a module from the extension root and the Python manifest does not enable the core client feature, it must not remain documented as an active Python surface.

Preferred outcome if client is out of scope: delete the orphan source and its stale docs/tests rather than wiring it up.

### Acceptance criteria

- no `client`/`client-tls` feature remains unless intentionally retained;
- no dead `client.rs` remains in the Python extension tree;
- no architecture document describes an uncompiled Python client as current;
- generic HTTP primitives remain available;
- default server and Python static-server functionality are unchanged.

---

# Track C — Remove the deprecated pre-runtime serving adapter

The current production architecture is:

```text
Server
  -> RuntimeState
  -> Service / StaticService
  -> canonical response
  -> transport conversion
```

The old `eggserve_core::service` adapter should not remain indefinitely if it only delegates into the new runtime with explicit test-owned context.

Inspect:

```text
crates/eggserve-core/src/service.rs
crate re-exports
legacy `ServeState`-based helpers
bin test-only `serve_connection()` helper
integration tests specifically preserving the adapter
architecture/docs references to the deprecated path
```

If no supported consumer requires the adapter:

1. remove `pub mod service` from `eggserve-core`;
2. remove the adapter implementation;
3. replace or delete tests that only verify the obsolete adapter;
4. modify bin tests to exercise the production `Server` / `StaticService` path rather than a private legacy Hyper loop;
5. remove `#[allow(deprecated)]` scaffolding made necessary only by the adapter;
6. update public API snapshot tests so the removed experimental/deprecated surface is not expected.

### Do not reimplement the same adapter under another name

If a test needs a local server, prefer constructing the actual runtime with a pre-bound listener or port 0. Do not create another test-only direct-Hyper serving path unless a specific low-level unit test requires Hyper itself.

### Acceptance criteria

- production and representative integration tests use the same runtime architecture;
- no deprecated pre-runtime serving module remains without a concrete supported consumer;
- no separate test-only serving stack is maintained merely for historical compatibility;
- static-server behavior remains unchanged.

---

# Track D — Reconcile public API/stability checks

Deletion must not leave stability tests asserting obsolete surfaces.

Inspect tests such as:

```text
public_api_consumers.rs
api_stability.rs
no_hyper_in_public_api.rs
Python test_public_api.py
Python boundary-hardening tests
```

Update snapshots/contracts to describe the intended API rather than preserving every historical alpha symbol.

Rules:

- stable-ish canonical primitives keep their current compatibility expectations;
- experimental/deprecated surfaces may be removed before 1.0;
- Python `eggserve.server` facade remains supported;
- internal callback/native implementation types do not become top-level APIs merely to satisfy old tests;
- no Hyper type may leak into the supported public API as a side effect of cleanup.

### Acceptance criteria

- stability tests pass with the intended post-cleanup surface;
- removed experimental/deprecated symbols are not treated as regressions;
- stable primitive behavior is unchanged;
- Python public namespace remains intentionally bounded.

---

# Track E — Targeted verification

Minimum verification after implementation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
```

If client features are removed, verify feature metadata no longer references them:

```sh
cargo metadata --no-deps --format-version 1
cargo tree -e features -p eggserve-core --no-default-features
```

Verify Python package behavior with the installed-wheel path:

```sh
bash scripts/test-python-wheel.sh
```

Run focused searches:

```sh
rg -n "client-tls|primitives::client|PyHttpClient|HttpClient|ClientError" \
  crates docs architecture README.md AGENTS.md

rg -n "eggserve_core::service|handle_request\(|deprecated.*service|pre-runtime" \
  crates docs architecture README.md AGENTS.md
```

Any remaining match must be either:

- historical plan text;
- a deliberate retained supported reference;
- unrelated ordinary English use of the word `client`.

Do not rewrite historical plans solely to remove old names.

---

## Final acceptance criteria

Plan 113 is complete when:

- EggServe has one clearly supported serving architecture;
- the deprecated service adapter is removed unless a concrete supported consumer blocks removal;
- the full HTTP client implementation is removed unless a concrete supported contract blocks removal;
- generic HTTP primitives remain intact;
- Python package sources and compiled/exported modules agree;
- API stability tests reflect intentional alpha/stable boundaries;
- no new feature or compatibility layer was added to compensate for deleted out-of-scope surfaces;
- routine Rust and installed-wheel tests pass.

## Rejection conditions

Reject the implementation if it:

- wires up the orphaned Python client merely to avoid deleting it;
- expands client features;
- removes canonical HTTP primitives used by custom servers;
- preserves the deprecated service adapter through a new alias;
- moves production tests onto a second direct-Hyper stack;
- changes filesystem confinement or static response semantics without a discovered bug;
- treats alpha experimental APIs as immutable 1.0 commitments without repository evidence.
