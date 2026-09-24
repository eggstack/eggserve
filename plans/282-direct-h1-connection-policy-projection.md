# Plan 282 — Direct H1 connection-policy projection and config decomposition

## Status

**PLANNED; BLOCKED on Plans 280–281 implementation.**

Plan 279 must already be closed.

## Purpose

Make the caller-owned H1 driver consume a narrow effective connection policy
instead of the full `RuntimeConfig`, while preserving all existing public
entry points as compatibility wrappers.

This is primarily an ownership/maintainability change. It should make direct
embedding easier to reason about and prevent unrelated listener/TLS/static
configuration from becoming accidental H1 connection authority.

## Current problem

`serve_http1_connection` currently receives `Arc<RuntimeConfig>`. That
single type contains settings for:

- listener bind;
- file streaming;
- TLS handshake validation;
- parser controls;
- total/idle/handler/body/write deadlines;
- body/target/header ceilings;
- service/tunnel admission;
- response policy;
- trusted-proxy policy.

Only part of that state is intrinsic to one H1 connection. Plans 280–281 add
explicit ownership semantics; continuing to thread the whole config through
every module would make those semantics harder to audit.

## Architecture target

Introduce a validated effective direct-H1 policy assembled once at the
embedding boundary.

Suggested decomposition:

```text
RuntimeConfig
  |
  +-- project + validate ------------------------+
  |                                             |
  v                                             v
Http1ParserPolicy                        H1ConnectionPolicy
  max_buf_size                            total lifetime
  max_headers                             header timeout
  mandatory timer                         idle ownership/budget
                                          handler/body/write ownership
                                          header/target/body semantic limits
                                          response policy
                                          trusted-proxy policy
                                          target-form policy (post-278)
                                          admission ownership reference
```

Exact type boundaries may differ, but the resulting connection driver must not
need bind/TLS-handshake/static-only settings.

## Track A — Public/source compatibility

Keep these existing entry points working with identical default behavior:

- `serve_http1_connection`;
- `serve_http1_connection_with_id`;
- `Server::builder()`;
- existing `RuntimeState` constructors.

They become projection wrappers over the new authority.

Add a new advanced direct entry point only where necessary, for example:

```rust
serve_http1_connection_with_policy(
    io,
    service,
    Arc<H1ConnectionPolicy>,
    context,
    runtime_state,
    shutdown,
)
```

Do not expose Hyper builder/config types.

## Track B — Mandatory parser policy

Promote the current private `Http1Config` concept into a clearly owned,
validated parser policy.

It must continue to require:

- parser buffer >= Hyper minimum;
- bounded parser buffer;
- non-zero/bounded header count;
- explicit Tokio timer whenever header read timeout is active;
- automatic Hyper Date disabled.

Plan 280 does not allow parser buffer/header-count ownership to become
External.

If header-read timeout remains mandatory, it belongs here or in the immediate
connection policy with no external state.

## Track C — Effective semantic policy

Project only the values the request pipeline actually needs:

- aggregate header bytes;
- request-target mode/limit;
- global body ceiling ownership;
- body deadline ownership;
- handler deadline ownership;
- response-write/idle ownership;
- total connection deadline;
- response privacy/error policy;
- trusted-proxy policy.

Use typed ownership from Plan 280 rather than re-checking sentinel numbers
throughout the pipeline.

Avoid passing both the raw `RuntimeConfig` and projected policy to the same
deep helper unless one is temporary during migration. The final state should
have one authority at each call site.

## Track D — Admission projection

Consume Plan 281's admission representation through `RuntimeState` or a
small typed admission view.

The connection pipeline should not know the original configured semaphore
numbers once runtime state is built. It should ask the state to admit or
observe External ownership.

Keep file-stream admission explicit because response conversion may still need
it.

## Track E — Validation

Create one deterministic projection/validation boundary.

Required properties:

- ordinary `RuntimeConfig::validate` behavior remains;
- default projection cannot fail after a successfully validated config;
- externally-owned policies do not require fake disabled scalar values;
- parser invariants cannot be bypassed;
- invalid combinations return `ServerError::Config`, not panic;
- high-level `ServerBuilder::build` validates before task creation;
- caller-owned entry points validate before Hyper/semaphore use.

Do not maintain a second drifting defaults table.

## Track F — Module signatures

Refactor deep modules toward the narrowest inputs:

- `driver.rs` gets parser/connection deadlines, not the whole config;
- `pipeline.rs` gets request/response semantic policy + runtime state;
- `request.rs` gets exact request ceilings/target mode;
- `response.rs` gets response policy/presentation authority;
- deferred-body code gets body deadline policy;
- forwarded policy gets only trusted-proxy policy plus connection context.

Do not turn this into a generic dependency-injection framework.

## Track G — Compatibility/core/H3

Audit all compatibility projection sites.

Requirements:

- core compatibility behavior remains unchanged;
- H2/H3 support tiers remain unchanged;
- H3 may keep its existing richer config projection if sharing the new narrow
  H1 policy would couple protocols incorrectly;
- no H1-only type leaks into primitives;
- no static filesystem setting moves into server connection policy.

## Track H — Guards

Add a structural guard that catches future re-expansion of direct H1
connection code onto unrelated config fields.

A reasonable guard may assert that the canonical connection driver no longer
accepts `RuntimeConfig` directly, while the public legacy wrappers may.

Do not write brittle line-number tests.

## Track I — Documentation

Update architecture docs to identify:

- raw operator config;
- validated H1 effective policy;
- runtime admission state;
- canonical service boundary.

Document that projection is an authority boundary, not merely a convenience
copy.

## Verification

At minimum:

```bash
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-server
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
bash scripts/check-supply-chain.sh
```

Retain all direct Tower/http-interop feature lanes and post-279 target-form
tests.

## Acceptance criteria

- [ ] Plans 280–281 behavior is represented in typed effective policy/state.
- [ ] existing direct APIs remain default-compatible.
- [ ] canonical H1 driver no longer depends on unrelated bind/TLS/static
      config.
- [ ] mandatory parser limits remain impossible to disable accidentally.
- [ ] defaults exist in one authority only.
- [ ] policy-aware validation occurs before runtime task construction.
- [ ] core/H2/H3/static behavior is unchanged.
- [ ] structural tests/guards prevent config authority from reconverging.
- [ ] full CI is green.

## Non-goals

- No new runtime behavior beyond Plans 280–281.
- No error presentation hook; Plan 283 owns that.
- No public configuration DSL.
- No H2/H3 unification solely for aesthetic symmetry.
- No publication.
