# Plan 248 — API-preserving maintainability convergence qualification and closure

## Purpose

Close Plans 242–247 with explicit evidence that the maintenance/convergence
work reduced duplication and fixed lifecycle/interop defects without changing
the existing API surface or capability set.

This plan is a qualification/closure gate. Production changes are not
authorized here except narrowly scoped fixes for defects introduced by the
242–247 implementation. If qualification exposes an unrelated pre-existing
defect, open a separate corrective plan.

## Preconditions

Required:

- Plan 243 direct-server shutdown/lifecycle corrective complete;
- Plan 244 H1 runtime authority convergence complete;
- Plan 245 static-service authority convergence complete;
- Plan 246 Python interop typing/internal-maintainability work complete or
  explicitly closed with a documented NO-GO/DEFER subtrack;
- Plan 247 leaf-surface/orphan-source/qualification cleanup complete.

## Evidence layout

Create a durable closure record, for example:

```text
release/plan-248-maintainability-convergence-closure.md
```

Record:

- baseline SHA: `673b6c60dab09d728b05d9e979be91bfc5417050`;
- final candidate SHA;
- exact Rust/Python toolchain versions;
- package/feature graph summaries;
- moved/deleted implementation inventory;
- public API compatibility evidence;
- Python symbol/type evidence;
- local validation commands/results;
- remote CI run IDs/URLs for the exact candidate SHA.

Do not describe line-count reduction as correctness evidence. Use it only as a
maintenance metric.

## Track A — Rust public API/source compatibility

### 1. Compile old-path fixtures

Build fixtures that exercise the existing documented public paths from the
baseline, including:

- `eggserve_core::primitives`;
- `eggserve_core::server`;
- direct `eggserve_primitives`;
- direct `eggserve_server`;
- direct `eggserve_static`;
- `eggnet_tls`;
- feature-gated Tower/http-interop/H2/H3/TLS paths presently documented.

Where feasible, retain baseline fixture source unchanged and compile it against
the candidate.

### 2. Public API diff

Use rustdoc JSON/public-api tooling if already available or reasonable to add
as a dev/release tool. Otherwise use explicit fixture inventories.

Classify every detected difference as:

- implementation-only;
- additive non-breaking;
- compatibility-preserved move/re-export;
- prohibited breaking difference.

Any prohibited breaking difference blocks closure.

Do not add a heavy production dependency for API diffing.

### 3. Type identity checks

Retain compile-time identity checks for compatibility re-exports where type
identity is part of the migration contract.

## Track B — Wire/capability parity

Run representative parity through both direct and compatibility construction.

Cover:

- H1 plain TCP;
- H1 TLS;
- H2/TLS;
- caller-owned/prebound transport;
- static GET/HEAD/range/conditional/listing;
- body buffer/stream/trailers;
- tunnel accept/deny;
- trusted proxy metadata;
- shutdown/drain;
- error representation and extra static headers.

H3 remains subject to its existing experimental qualification; run its
established deterministic suite but do not promote it.

No existing successful capability may disappear solely because ownership moved.

## Track C — Lifecycle corrective proof

Retain deterministic Plan 243 regressions and explicitly record:

- immediate shutdown cannot be missed;
- shutdown concurrent with accepted-task startup cannot be missed;
- `wait()` accounts for accepted runtime-owned tasks;
- repeated shutdown is safe;
- admission/task state returns to baseline.

Run the relevant tests under a repetition/stress harness where practical to
guard against scheduler-sensitive false passes.

## Track D — Static authority proof

Record the before/after implementation ownership matrix.

Prove:

- path/filesystem/planner/service behavior has one static authority;
- core compatibility static APIs still behave identically;
- opened file capabilities are never replaced by pathname reopen;
- directory listing and metadata/error policy behavior survive delegation.

## Track E — Python compatibility and typing

Against a built/installed wheel:

- compare public symbol manifest to baseline;
- run all existing Python compatibility/low-level/async tests;
- run static type-check fixtures;
- verify stub and `py.typed` wheel composition if Plan 246 enabled typed
  package status;
- verify CPython 3.11 abi3 baseline and current 3.14 compatibility;
- confirm stock static fast path still avoids Python callback dispatch.

No Python import or constructor regression is acceptable.

## Track F — dependency and feature graphs

Capture `cargo metadata`/`cargo tree -e features` for:

- primitives default and optional features;
- server default, `http2`, `tls`, and combinations retained;
- static default/internal Python feature;
- core default, H2/TLS, H3/TLS, Tower/http-interop;
- bin default/feature sets;
- Python wheel closure.

Confirm:

- primitives remains runtime/transport neutral;
- server does not depend upward on core/static;
- static remains the sole fs authority;
- H3 remains isolated from default graphs;
- no orphan duplicate implementation dependency remains;
- reserved/inert features do not pull unjustified dependencies;
- rustls security floors remain satisfied.

## Track G — quality and security matrix

Run the current full repository matrix:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
bash scripts/test-python-wheel.sh
```

Also run:

- direct leaf-crate test commands from Plan 247;
- TLS/H2/H3 focused suites;
- filesystem race/adversarial suites;
- proxy interoperability where available;
- fuzz/corpus replay used by the current deep verification mode;
- platform qualification workflow for macOS/Windows where the repository
  already supports it.

No new absolute performance threshold is required. Run representative Plan 241
workloads only as a sanity check that convergence did not create an obvious
material regression.

## Track H — maintenance result accounting

Record mechanically:

- production Rust files deleted or converted to facades;
- duplicate authority modules removed;
- remaining intentional core composition modules;
- orphan source count (must be zero under the new check);
- direct test ownership by crate;
- Python binding module decomposition;
- feature classification table.

This is an auditability record, not a target to maximize deletion.

## Track I — remote CI provenance

Push the candidate and require successful GitHub Actions for the exact SHA.

At minimum require the repository's normal:

- rust;
- supply-chain;
- Python wheel;

jobs, plus any platform/release qualification explicitly triggered for this
campaign.

Record exact SHA, run IDs, URLs, timestamps, and job conclusions.

If a later documentation-only commit records those results, distinguish the
CI-verified candidate SHA from the final metadata-record SHA exactly as prior
evidence plans do.

## Acceptance criteria

- [ ] no prohibited Rust public API/source compatibility differences;
- [ ] no Python public symbol/signature regression;
- [ ] all existing capability paths remain functional;
- [ ] Plan 243 lifecycle regressions are deterministic and green;
- [ ] one shared H1 implementation authority remains;
- [ ] one static service implementation authority remains;
- [ ] orphan production Rust sources are structurally rejected;
- [ ] feature documentation/effects match reality without removing accepted
      names;
- [ ] direct crates independently qualify their authorities;
- [ ] full routine/security/package/wheel/platform matrix is green;
- [ ] dependency graph preserves layering/security floors;
- [ ] representative performance/resource sanity shows no material regression;
- [ ] exact candidate SHA has successful remote CI evidence;
- [ ] roadmap/architecture docs match the final implementation.

## Closure decision

If all criteria pass, mark Plans 242–248 complete and do not open another
corrective merely to remove remaining composition glue that is explicitly
classified and non-duplicative.

If a criterion fails:

- lifecycle/correctness failure → open a narrow production corrective;
- API compatibility failure → restore compatibility before closure;
- performance-only concern → reproduce and scope separately;
- typing incompleteness → defer `py.typed`, do not weaken runtime work;
- protocol-specific failure → fix within that protocol authority without
  broadening support tier.

## Non-goals

Do not use closure to add features, remove compatibility paths, promote H2/H3,
or perform unrelated dependency/version churn.
