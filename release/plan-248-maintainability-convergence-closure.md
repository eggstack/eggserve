# Plans 243–248 maintainability convergence closure

Status: COMPLETE, WITH TWO CLAIMS SUPERSEDED BY PLANS 249–250 (see pointer
below; all other results remain closed).

Baseline SHA: `673b6c60dab09d728b05d9e979be91bfc5417050`.

## Implemented authority changes

- Plan 243: durable direct-server shutdown state, runtime-owned accepted-task
  `JoinSet` draining, idempotent shutdown, and deterministic lifecycle tests.
- Plan 244: compatibility H1 entry points project configuration and shared
  semaphore/ops state into `eggserve-server`; H2/TLS/proxy composition remains
  compatibility-owned.
- Plan 245: `eggserve-static::StaticService` owns static request planning,
  file/directory responses, metadata, errors, and listings; core retains a
  compatibility wrapper.
- Plan 246: wheel-shipped `lowlevel.pyi` and `py.typed`, installed-wheel
  mypy smoke fixture, and isolated PyO3 registration module.
- Plan 247: removed the orphan `primitives/runtime_limits.rs`, added a
  production Rust module reachability check, and reconciled accepted inert
  direct-crate feature names.

## Compatibility evidence

The existing compatibility paths remain covered by the core parity and
authority fixtures. The installed Python wheel retains the public facade,
native fast path, TLS path, low-level sync/async substrate, subprocess helper,
and canonical symbol surface. The wheel includes `eggserve/lowlevel.pyi` and
`eggserve/py.typed`; the representative strict mypy fixture passes.

## Validation record

Toolchain: Rust 1.89/MSRV lane and CPython 3.14 building the CPython 3.11
abi3 wheel. Exact versions and command output for the final candidate are
recorded below before closure.

Passed focused checks:

- `cargo test -p eggserve-server` — 81 passed;
- `cargo test -p eggserve-static` — 345 passed, 2 ignored;
- direct H1 parity and service convergence — 19 passed;
- static authority conformance — 7 passed;
- installed-wheel typing and Python suite — 804 passed;
- `python3 scripts/check-crate-topology.py`;
- direct server compatibility feature checks and primitives interop feature
  check.

Final routine matrix, package checks, focused protocol checks, and remote CI
provenance are recorded by the final candidate below.

## Final candidate and remote CI

Final candidate SHA: `3fb59e4560b74407b7faed3a09aaae5974d3d36a`.

Remote CI: [run 35602644725](https://github.com/eggstack/eggserve/actions/runs/35602644725) — success.

The final candidate passed local formatting, conformance/topology/metadata,
workspace and feature-matrix tests/clippy, the excluded Python crate check,
wheel typing and Python tests, supply-chain audits, and layered package
verification before the exact remote CI run passed.

## Supersession pointer (Plans 249–250)

The original CI result above remains valid for its candidate, but two closure
claims are superseded and corrected by Plans 249–250 without erasing this
evidence: (1) the topology gate proved direct H1 delegation existed but did
not prove normal `WireProtocol::Auto` connections could not still execute
core's private Hyper H1 pipeline; (2) the matrix did not detect detached
per-connection broadcast-forwarder tasks outliving normal connection
completion. Plan 249 moves `Auto` classification before Hyper-service
construction (H1 delegates the replayable stream to `eggserve-server`; core
keeps H2-only execution) and replaces detached forwarders with
`run_with_connection_shutdown` structured under the connection task; Plan 250
requalifies and records exact-SHA CI. See
`release/plan-250-h1-authority-lifetime-corrective-closure.md`.
