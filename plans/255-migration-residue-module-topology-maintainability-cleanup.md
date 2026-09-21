# Plan 255 — Migration-residue, module-boundary, and topology-checker maintainability cleanup

## Purpose

Close the remaining maintainability residue exposed by the post-Plan-250 review
without changing EggServe's public API, capability set, security model, crate
ownership, or protocol support tiers.

Planning baseline:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
```

This plan covers three related maintenance concerns:

1. extraction/decomposition residue such as broad unused-import suppressions,
   copied import preambles, and historical migration commentary in production
   code;
2. very large multi-responsibility modules whose review cost can be reduced by
   internal-only decomposition where a natural boundary exists;
3. `scripts/check-crate-topology.py`, which is now itself a large executable
   architecture specification and should be easier to review/test without
   weakening any rule.

It also clarifies the Rust library consumption guidance discovered in the
review: direct H1 consumers should have a clearly documented leaf-crate path,
while consumers needing the current H2/TLS/listener/proxy/H3 composition should
use the compatibility umbrella.

## Constraints

- No public Rust/Python item removal, rename, signature change, or type-identity
  change.
- No product feature or support-tier change.
- No ownership move that conflicts with Plans 215–250.
- No new broad production dependency.
- No crate split solely to make files shorter.
- Do not split security-sensitive static filesystem code merely to satisfy a
  size metric.
- Do not weaken or delete a topology/security check because it is inconvenient.
- Do not rewrite historical release/plan evidence.
- Prefer invariant-focused comments in production; historical chronology stays
  in architecture/release/plan documents.
- Plan 253 owns semantic connection-authority convergence. Plan 255 must not
  independently move H1/H2 authority.

## Track A — remove broad unused-import suppressions where migration is complete

Audit production Rust modules for broad suppressions such as:

```rust
#![allow(unused_imports)]
```

Known review examples include:

- `crates/eggserve-core/src/server/runtime.rs`;
- `crates/eggserve-python/src/server/request_bridge.rs`;
- `crates/eggserve-python/src/server/response_bridge.rs`.

For each:

1. remove the broad suppression;
2. trim imports to what the module actually uses;
3. if a narrow import is intentionally present only under a feature/platform,
   gate it correctly rather than suppressing the whole module;
4. retain a local `#[allow(...)]` only when there is a concrete unavoidable
   generated/macro/platform reason and document that reason.

Clippy with `-D warnings` should become the authority again.

Do not turn import cleanup into behavior refactoring.

## Track B — deduplicate Python binding import and bridge scaffolding

The decomposed Python server bridge modules currently retain copied import
preambles from their former monolithic implementation.

Review:

- `request_bridge.rs`;
- `response_bridge.rs`;
- `body_bridge.rs`;
- `tunnel_bridge.rs`;
- `sync_handler.rs`;
- `async_handler.rs`;
- `static_responder.rs`;
- `runtime.rs`.

Goals:

- each module imports only its direct dependencies;
- common internal-only aliases/helpers live in the smallest existing parent
  module that already owns them;
- no wildcard-like shared prelude is introduced merely to hide dependency
  ownership;
- the PyO3 registration module remains the single registration authority;
- no Python-visible class/function moves.

Prefer explicit imports over a broad internal `prelude::*` if the latter
would obscure which leaf crate a bridge depends on.

## Track C — production-comment hygiene

Audit touched modules for comments whose primary value is historical chronology
rather than explaining a current invariant.

Replace comments of the form:

```text
Plan N moved X in Plan M ...
```

with concise invariant/ownership explanations when the plan number is not
needed to understand current behavior.

Keep plan references when they provide essential provenance for a subtle
security/compatibility decision, especially:

- public compatibility behavior;
- unsafe/platform boundaries;
- blocked protocol capabilities;
- security advisory floors;
- intentionally inert feature names.

Do not bulk-delete all plan references.

Historical detail remains available in `plans/`, `release/`, and
`architecture/`.

## Track D — large-module responsibility audit

Do not use line count as a correctness metric. Use it only to identify modules
where distinct responsibilities are already visible.

Review at minimum:

- `crates/eggserve-bin/src/args.rs`;
- `crates/eggserve-h3/src/adapter.rs`;
- `crates/eggserve-python/src/lib.rs`;
- `scripts/check-crate-topology.py`.

Connection-pipeline modules are handled by Plan 253 and must not be independently
reorganized here until its ownership classification is complete.

For each candidate, answer:

- does the file own more than one independently testable responsibility?
- can a private submodule split preserve all public paths/type identity?
- will a split make feature/platform dependencies clearer?
- does splitting increase cross-module state coupling or make security
  invariants harder to review?

Only decompose when the first three answers are yes and the fourth is no.

### Explicit non-targets

The review identified large static-filesystem/platform modules, including the
Windows confinement implementation and static planner. Do not split these just
because they are large. Their locality may be a security-review advantage.

A separate plan is required if there is evidence that their current structure
causes correctness or audit failures.

## Track E — Python native binding `lib.rs` cleanup

`crates/eggserve-python/src/lib.rs` is still large after Plan 246's first
decomposition.

Inventory what remains and move internal-only coherent groups to existing/new
private submodules when that can be done mechanically.

Likely categories to consider:

- canonical primitive wrappers;
- static-resolution/planning wrappers;
- validation functions;
- method/version/header/connection/request wrappers;
- module registration stays in `registration.rs`.

Requirements:

- identical PyO3 class/function names;
- identical module registration;
- identical exception hierarchy;
- no change to `_native.pyi` except corrections required by Plan 252;
- no new Python import path.

Do not split a wrapper family when doing so makes shared invariant validation
less obvious.

## Track F — CLI argument module audit

Review `eggserve-bin/src/args.rs` for clean internal boundaries such as:

- argument type definitions;
- parse/normalization;
- validation;
- projection into runtime/static/TLS configuration;
- help/version rendering;
- tests if embedded in the module.

If coherent private submodules can reduce review burden while preserving every
CLI flag/default/error behavior, perform the split.

Do not change Clap/argument behavior, flag names, defaults, exit codes, or
error strings relied on by tests without a separate compatibility reason.

If the file is large primarily because its tests intentionally live beside the
parser and a split would worsen auditability, record NO-CHANGE with rationale.

## Track G — H3 adapter audit

Review `eggserve-h3/src/adapter.rs` for private separable responsibilities
already represented conceptually by the crate's `request.rs`, `response.rs`,
`endpoint.rs`, `quic.rs`, and `tunnel.rs`.

Only move code when it clearly belongs to an existing authority module and the
move does not:

- change public exports;
- change H3 behavior;
- change the currently blocked/experimental support tier;
- alter dependency versions;
- reopen Plan 192/193 promotion work.

If the adapter's size is primarily due to necessary orchestration, retain it.

## Track H — refactor topology checker internally without weakening semantics

`scripts/check-crate-topology.py` currently combines dependency-graph policy,
crate authority rules, source inventories, Python/package checks, and
migration-specific negative markers in one large file.

Refactor it into reviewable internal sections/modules while keeping the same
single user-facing command:

```sh
python3 scripts/check-crate-topology.py
```

Acceptable approaches include a small `scripts/topology_checks/` package with
the entry script delegating to:

- dependency/feature graph checks;
- crate/module inventory checks;
- H1/static/H3 authority checks;
- Python/frontend topology checks;
- shared reporting/helpers.

Do not add a third-party Python dependency.

### Rule-preservation inventory

Before refactoring, create a machine-reviewable inventory of every existing
check function/rule family.

At minimum preserve rules for:

- primitives transport neutrality;
- neutral TLS dependency isolation;
- direct H1 authority;
- service/tunnel authority;
- static/path/filesystem authority;
- H3 isolation/facade behavior;
- Python frontend dependency rules;
- orphan production Rust sources;
- accepted inert feature names;
- Plan 249 no-core-H1/no-detached-forwarder assertions;
- compatibility module inventory.

After refactoring, compare the baseline and candidate rule inventory.

## Track I — topology-checker self-tests / mutation fixtures

The checker currently uses source markers for some negative architecture
assertions. Add lightweight self-tests or fixture-driven mutation tests for
the most brittle rules.

At minimum prove failure when representative fixtures contain:

- forbidden core H1 Hyper builder/connection execution;
- forbidden static resolver authority in core;
- forbidden upward dependency from primitives;
- unclassified new core production source;
- prohibited direct-crate capability dependency;
- detached shutdown-forwarder pattern guarded by Plan 249;
- missing required Python typed artifact if that check remains here.

Do not leave intentionally broken repository source. Tests should operate on
fixture text/temp trees or isolated helper functions.

The checker must remain fast enough for routine CI.

## Track J — clarify Rust consumption profiles

Update the current user/maintainer docs so there are two explicit supported
architectural profiles without changing support tiers:

### Direct generic H1 substrate

Use:

```text
eggserve-primitives
eggserve-server
(+ eggserve-static when static serving is needed)
```

This is the leaf architecture for consumers that need the generic H1 runtime
and canonical service boundary.

### Compatibility/multiprotocol composition

Use:

```text
eggserve-core::server
```

when the consumer needs current compatibility-owned composition such as H2,
extended TLS/listener/proxy integration, or the optional H3 facade.

Document clearly that:

- `eggserve-core` is not deprecated/removed by this campaign;
- direct `eggserve-server` does not gain H2/TLS simply because compatibility
  feature names are accepted;
- H2/H3 remain experimental;
- the canonical application types remain Hyper-free.

Likely docs:

- `README.md`;
- `docs/public-api-boundary.md`;
- `docs/library-capability-matrix.md`;
- `docs/downstream-app-server.md`;
- `architecture/crate-topology.md`.

Avoid duplicating large tutorials across all files; use one authoritative
explanation and links.

## Required qualification

Run:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
```

Run focused package/CLI/H3/Python suites for any module actually decomposed.

For topology-checker refactoring, run its self-tests plus temporary negative
fixtures and confirm routine CI invocation remains unchanged.

## Acceptance criteria

- [ ] broad post-migration unused-import suppressions are removed where no
      concrete justification remains.
- [ ] touched bridge modules have minimal explicit imports.
- [ ] production comments emphasize current invariants over obsolete migration
      chronology.
- [ ] any large-module split has a documented responsibility boundary and no
      public behavior change.
- [ ] Python native binding registration/names/exceptions remain identical.
- [ ] CLI flags/defaults/errors remain compatible.
- [ ] H3 capability/dependency/support tier is unchanged.
- [ ] topology-checker rule inventory is preserved.
- [ ] representative negative self-tests prove important checker rules still
      fail closed.
- [ ] `python3 scripts/check-crate-topology.py` remains the stable entrypoint.
- [ ] direct-H1 versus compatibility-multiprotocol Rust usage is documented
      unambiguously.
- [ ] routine Rust/Python structural qualification is green.

## Stop conditions

Do not perform a decomposition if it:

- requires a public move/rename;
- creates circular or upward crate dependencies;
- hides a security invariant behind more layers;
- requires a new production dependency;
- conflicts with Plan 253 ownership classification;
- changes CLI/Python behavior;
- reopens H2/H3 feature/promotion scope.

Record NO-CHANGE/DEFER for that candidate instead.
