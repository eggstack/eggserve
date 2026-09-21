# Plans 243–248 maintainability convergence closure

Status: qualification record in progress.

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
provenance are appended here at closure.

## Final candidate and remote CI

Final candidate SHA: pending final commit.

Remote CI run IDs/URLs and conclusions: pending push.

The status changes to complete only after the exact final candidate has green
local routine/security/package checks and successful remote CI.
