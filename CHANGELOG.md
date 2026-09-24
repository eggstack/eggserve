# Changelog

## 0.2.3 — release candidate; crates.io publication pending

- Moved the optional HTTP interop and Tower adapter implementation to
  `eggserve-server`, where direct H1 consumers can enable `http-interop` or
  `tower` without depending on `eggserve-core`, `eggserve-static`, or PHF.
- Kept the historical core adapter paths as compatibility re-exports and
  forwarded core's adapter features to the server. Core remains the
  compatibility/static umbrella and keeps its static-serving dependencies.
- No static-serving, H1 transport, lifecycle, H2/H3, TLS, or Python runtime
  behavior changed. The adapters remain experimental at their existing tier.
- Workspace and Python source metadata are synchronized at 0.2.3; no Python
  wheel release is included.

## 0.2.2 — published

- Repaired the optional `http-interop`/`tower` feature path with a core-owned
  `HttpRequestBody` adapter after the canonical `RequestBody` moved to the
  primitives crate and made the former external-trait/foreign-type
  implementation illegal.
- Preserved canonical `RequestBody` and direct H1 runtime contracts. Adapter
  and server APIs remain experimental; routine CI now covers the advertised
  adapter feature profiles and Axum 0.8 composition.
- Python runtime behavior is unchanged. This synchronized source version does
  not constitute a Python wheel release.

## 0.2.1 — published

- Added `eggserve-server::ServerHandle::into_parts()` with cloneable
  `ServerControl` and typed, cancellation-safe `ServerCompletion` supervision.
- Propagated top-level and escaping runtime-owned connection-task panic or
  cancellation through `ServerError::Terminal`; legacy `wait(self) -> ()`
  remains source-compatible.
- Made `Duration::ZERO` an explicit opt-out for the total connection lifetime.
  The 60-second default and independent timeout, admission, and shutdown
  protections remain unchanged. The existing Python `Server` timeout
  parameter maps zero to the same opt-out; its default is unchanged.
- Added a leaf-crate-only prebound-listener and supervised keep-alive fixture.
- Generalized layered crate package verification to derive release versions
  from Cargo metadata.

Published from implementation candidate
`466cf6301f20c7202f696c495e6eb8d5e74664be`. The registry-only consumer proof
resolves `eggserve-server 0.2.1` and passes the supervised keep-alive smoke;
see [the Plan 272 closure evidence](release/plan-272-downstream-embedding-qualification-closure.md).
