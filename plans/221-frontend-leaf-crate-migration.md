# Plan 221 — Migrate first-party frontends off eggserve-core

## Purpose

Make the repository's own binary and Python extension consume the canonical leaf crates directly. This converts `eggserve-core` into a real compatibility layer instead of the implementation path used by first-party products.

## Goals

- Migrate `eggserve-bin` to `eggserve-primitives`, `eggserve-server`, `eggserve-static`, `eggnet-tls`, and optional `eggserve-h3` as appropriate.
- Migrate `eggserve-python` to the same canonical layers.
- Verify and remove the apparently unused direct `eggserve-python -> eggserve-bin` dependency.
- Reduce Python-side duplicated validation by delegating shared runtime validation to canonical Rust types.
- Keep public CLI/Python behavior unchanged.

## Non-goals

- No new Python framework semantics.
- No H2/H3 Python expansion.
- No CLI feature expansion.
- No deletion of `eggserve-core`; Plan 225 owns final facade closure.

## Work

### 1. Binary migration

Inventory all `eggserve_core::` imports in `eggserve-bin` and map them to their canonical owners.

Expected mapping:
- runtime/service/config -> `eggserve-server`
- static service/config -> `eggserve-static`
- neutral policy/request/response -> `eggserve-primitives`
- TLS -> `eggnet-tls`
- H3 -> optional `eggserve-h3`

Keep CLI-only concerns in the binary crate.

### 2. Python migration

Map PyO3 bridge imports the same way.

Preserve:
- canonical request/response conversion,
- opened-file capability semantics,
- callback semaphore/backpressure,
- lifecycle/tunnel ownership,
- byte-fidelity views,
- TLS configuration behavior,
- static responder behavior.

Avoid using compatibility-only constructors to move resolved files. Add narrow leaf-crate APIs only when they are genuinely reusable.

### 3. Remove unused Python -> bin dependency

The review found a direct `eggserve-bin` dependency in `crates/eggserve-python/Cargo.toml` but no source references.

Verify with:
- cargo metadata,
- source search,
- `cargo machete` or equivalent local dependency-use check if available,
- wheel build without the dependency.

Remove it if confirmed unused.

### 4. Canonicalize Python runtime validation

Python currently repeats many scalar validations for max connections, parser ceilings, body limits, timeouts, etc.

Construct canonical `SharedRuntimeValues` / `RuntimeConfig` and translate structured violations into Python `ValueError` rather than maintaining an independent table.

Keep Python-only validation local:
- `public=True` bind acknowledgement,
- callback concurrency,
- Python handler shape,
- Python-specific response representation,
- facade-specific TLS file argument pairing.

### 5. Feature graph cleanup

After migration:
- remove core dependency from first-party frontends if possible,
- verify minimal CLI build does not pull optional H3/Tower/Python-only deps,
- verify Python wheel closure contains only required leaf crates.

### 6. Documentation

Update architecture docs to make the distinction explicit:
- direct crates are implementation authorities,
- core is compatibility facade,
- first-party frontends prove the direct architecture.

## Tests

- CLI unit/integration tests,
- installed binary smoke,
- Python wheel build/install/smoke,
- Python server callback tests,
- static serving tests,
- TLS tests,
- package metadata checks,
- dependency graph assertions,
- compile test ensuring first-party frontends contain no `eggserve_core::` imports after completion.

## Migration strategy

Migrate binary first, then Python. This keeps one known-good first-party direct consumer before touching PyO3.

## Rollback

If a leaf API gap appears, add the minimum neutral API to its correct owner. Do not restore implementation to core solely to satisfy a frontend.

## Acceptance criteria

- `eggserve-bin` builds without `eggserve-core`.
- `eggserve-python` builds without `eggserve-core`, unless one explicitly documented compatibility-only blocker remains.
- Unused Python -> bin dependency is removed if confirmed.
- Shared runtime validation has one Rust authority.
- CLI and wheel behavior remain unchanged.
- Dependency graphs are smaller or equal and ownership is clearer.
