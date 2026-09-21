# Plan 247 — Leaf-crate surface, orphan-source, and qualification cleanup

## Purpose

Clean up repository artifacts and direct-crate exposure that no longer match
the post-225 architecture, while preserving every accepted public feature name
and runtime capability.

The Plan 242 review identified three maintenance defects:

1. `crates/eggserve-primitives/src/primitives/runtime_limits.rs` is an
   orphaned, uncompiled copy of the runtime-limit authority. It is not declared
   by `primitives/mod.rs` and references ownership/dependencies that no
   longer belong in `eggserve-primitives`.
2. several direct-crate feature declarations can imply capability that the
   direct crate does not actually expose; notably `eggserve-server` carries
   `http2`/`tls` feature edges while its documented direct runtime remains
   H1-only, and `eggserve-primitives` declares an empty `http-interop`
   feature while the compatibility adapter currently lives in core.
3. much of the qualification proving direct authority still lives under
   `eggserve-core/tests`, making the leaf crates less independently
   self-verifying than their architecture implies.

This plan is cleanup/qualification work. It must not remove accepted feature
names or capabilities.

## Work

### 1. Delete the orphan runtime-limit source

Verify that
`crates/eggserve-primitives/src/primitives/runtime_limits.rs` is not compiled,
imported, included by generated tooling, or intentionally retained as fixture
text.

Then delete it.

The single runtime-limit authority remains
`eggserve-server/src/runtime_limits.rs`; core continues to facade that
authority through `eggserve-core/src/runtime_limits.rs`.

Record the deletion in architecture/release evidence so future reviewers do
not mistake it for removed public functionality.

### 2. Add an orphan Rust-source structural check

Extend repository tooling with a conservative check for production Rust source
files that are not reachable from a crate's declared module tree.

The check must:

- understand `lib.rs`/`main.rs`, `mod foo;`, inline modules, and
  conventional `foo.rs` / `foo/mod.rs`;
- exclude tests/examples/benches/build scripts and explicitly documented
  generated/fixture source;
- avoid false positives for cfg-gated modules that are still declared;
- report the path and owning crate clearly.

It may be integrated into `check-crate-topology.py` or a small dedicated
script called by the same CI/verify entry points.

Do not create a fragile blanket “every .rs file must appear as a text string”
test.

### 3. Audit direct-crate feature semantics

For every published crate, inventory feature names, actual dependency changes,
and actual public/runtime behavior.

Focus on:

- `eggserve-server/http2`;
- `eggserve-server/tls`;
- `eggserve-primitives/http-interop`;
- compatibility forwarding features in `eggserve-core`/`eggserve-bin`;
- `python-bindings-internal`;
- H3 feature isolation.

Classify each feature:

- ACTIVE CAPABILITY;
- COMPATIBILITY/RESERVED FORWARDER;
- INTERNAL;
- OBSOLETE BUT RETAINED.

### 4. Preserve names while correcting misleading effects

Do not remove or rename existing published feature flags under this campaign.

For a feature that is intentionally inert/reserved:

- document that status directly in Cargo comments and user-facing feature docs;
- if it unnecessarily pulls a production dependency while exposing no
  capability, remove only the unnecessary dependency activation while leaving
  the feature name accepted;
- add a feature-graph test proving its resolved dependency effect.

For a feature made meaningful by Plan 244 convergence, update the
classification accordingly rather than preserving stale “inert” documentation.

Do not silently promote H2/TLS support in the direct crate without the Plan 244
architecture actually providing it.

### 5. Move authority tests to owning leaf crates

Migrate or reconstruct implementation-authority tests so each direct crate can
validate its own contract independently.

#### eggserve-server

Own direct tests for:

- high-level direct server lifecycle/shutdown (Plan 243);
- H1 request/body/response execution;
- timeouts/admission/max-requests;
- caller-owned transport;
- tunnel execution;
- direct configuration validation.

#### eggserve-static

Own tests for:

- static service behavior;
- planner semantics;
- directory listing;
- capability continuity;
- confinement integration.

#### eggserve-primitives

Own tests for canonical value construction/normalization and no runtime
dependencies.

#### eggnet-tls / eggserve-h3

Keep their existing authority tests and add only gaps discovered by the audit.

### 6. Keep compatibility/cross-layer tests in core

Do not blindly move everything out of core.

Core should continue to own tests that are specifically about:

- old compatibility paths;
- direct-vs-core type identity;
- direct-vs-core wire parity;
- H2/TLS/H3 composition;
- feature forwarding;
- `ServeConfig` compatibility;
- downstream consumer migration compatibility.

The goal is ownership clarity, not fewer tests.

### 7. Package documentation

Update crate READMEs/architecture docs so a crates.io user can answer:

- which crate should I depend on?
- what protocols does that crate itself serve?
- which feature flags are capability-bearing versus compatibility/reserved?
- when is `eggserve-core` still the right umbrella?

Do not change the high-level product boundary.

## Tests and gates

Add direct package commands to CI where useful rather than relying only on
workspace aggregation.

Required checks include:

```sh
python3 scripts/check-crate-topology.py
cargo check -p eggserve-primitives --all-targets
cargo test -p eggserve-primitives
cargo check -p eggserve-server --all-targets
cargo test -p eggserve-server
cargo test -p eggserve-static
cargo test -p eggnet-tls
cargo test -p eggserve-h3
cargo check -p eggserve-server --features http2
cargo check -p eggserve-server --features tls
```

Adjust the last feature checks to the final classifications after Plan 244.

Use `cargo tree -e features` or metadata-based assertions to retain the
resolved feature evidence.

## Acceptance criteria

- [ ] orphaned primitives runtime-limit source is deleted;
- [ ] CI rejects future orphan production Rust sources;
- [ ] every published feature is classified truthfully;
- [ ] no accepted feature name is removed;
- [ ] inert/reserved features do not pull unjustified dependencies;
- [ ] direct crates own tests for their own implementation authorities;
- [ ] core retains compatibility/cross-protocol parity tests;
- [ ] crate docs match actual direct capabilities;
- [ ] no public API/capability/support-tier regression.

## Non-goals

Do not rename crates, remove `eggserve-core`, split a new capability-fs crate,
or use feature cleanup as a vehicle for a breaking release.
