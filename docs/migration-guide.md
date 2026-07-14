# Migration Guide

This document covers migration paths for superseded APIs in eggserve. It is
intended for downstream consumers upgrading to releases that include canonical
HTTP types (Plans 047–049).

## Legacy → Canonical Type Mapping

### ReadOnlyMethod → Method

`ReadOnlyMethod` (GET/HEAD only) remains stable. `Method` (standard +
extension) is the canonical type for new code.

| Legacy | Canonical | Change |
|--------|-----------|--------|
| `ReadOnlyMethod::Get` | `Method::get()` | Same value, richer API |
| `ReadOnlyMethod::Head` | `Method::head()` | Same value, richer API |
| `validate_method("GET")?` | `Method::new("GET")?` | Unified constructor |

**Migration**: Replace `ReadOnlyMethod` with `Method` in new code. Existing
`ReadOnlyMethod` usage continues to work. `Method` supports extension methods
(e.g. `PURGE`) and provides `is_safe()`, `is_idempotent()`,
`permits_static_resolution()`.

### validate_request_target() → RequestTarget::parse()

| Legacy | Canonical | Change |
|--------|-----------|--------|
| `validate_request_target("/path")?` | `RequestTarget::parse("/path")?` | Typed errors, query support |

**Migration**: `RequestTarget::parse()` returns a typed `RequestTarget` with
`.path()` and `.query()` accessors. Error variants are more specific (Empty,
AbsoluteUri, AuthorityForm, AsteriskForm, ContainsWhitespace, NotOriginForm).

### Response planning types

The existing response planning types (`StaticResponsePlan`, `BodyPlan`,
`HeaderMapPlan`, `ResponseStatus`) remain stable. Canonical response types
(`StatusCode`, `Response`, `normalize_response`) are a parallel API for
constructing transport-independent responses.

| Use Case | Existing | Canonical |
|----------|----------|-----------|
| File response planning | `plan_file_response()` | N/A (planner is stable) |
| Custom response construction | `ResponsePlan` namedtuple (Python) | `Response::builder()` (Rust) |
| Status code | `ResponseStatus` (u16 newtype) | `StatusCode` (validated, classified) |

### Python header representation

| Legacy | Canonical | Limitation |
|--------|-----------|------------|
| `Response.headers: HashMap` | `HeaderBlock: Vec<HeaderField>` | HashMap loses duplicates |

**Migration**: Python handlers using `Response(headers={"Set-Cookie": "a=1"})`
cannot represent duplicate headers. For duplicate headers, use the
static-responder path which preserves duplicates through `HeaderMapPlan`.

## StatusCode Range Change

`StatusCode` now only accepts values in the 100–999 range (three-digit HTTP
status codes). Values below 100 (0–99) are no longer valid.

| Before | After | Impact |
|--------|-------|--------|
| `StatusCode` accepted 1–999 | `StatusCode` accepts 100–999 | `StatusCode::new(0)` through `StatusCode::new(99)` now return `Err(InvalidStatus)` |

This aligns with HTTP/1.1 syntax requirements: status codes are always
three-digit integers. Values below 100 are not defined by HTTP/1.1 and have no
semantic meaning in eggserve's response pipeline.

**Migration**: If you were using status codes below 100, replace them with
appropriate three-digit codes. The `normalize_metadata()` function enforces this
range for all response producers.

## Deprecation Policy

Deprecated stable items remain functional for at least one minor release after
deprecation is announced. Removal requires explicit release notes and migration
guidance.

### Currently Deprecated

None. All legacy APIs remain stable and functional.

### Internally Retained (not for downstream use)

| Item | Location | Reason |
|------|----------|--------|
| `ResolvedFile::into_std_file()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |
| `ResolvedFile::into_parts()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |
| `ResolvedFile::from_parts()` | `primitives::secure_root` | Python bindings only; behind `python-bindings-internal` feature |

These methods are disabled by default and are not part of the public contract.

## Breaking Change Policy

Pre-1.0, minor releases may break stable APIs only with explicit release notes
and migration guidance. Patch releases must not break stable APIs. Enum variant
additions to stable enums are breaking changes.
