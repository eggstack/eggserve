# Changelog

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
