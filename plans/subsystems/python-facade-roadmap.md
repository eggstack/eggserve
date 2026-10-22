# Python Facade Roadmap

Status: proposed

Long-term references:

- `plans/000-long-term-specification.md#2` (Python `http.server`-shaped facade; handler-only `lowlevel` substrate)
- `plans/001-terminology-and-domain-model.md` (`Request`, `Response`, `RequestBody`, `Service`)
- `plans/002-long-term-roadmap.md` (phase 4)

Related ADRs:

- none yet (no accepted ADR governs the Python bridge boundary)

## 1. Purpose and ownership boundary

Owns the Python-visible surface of EggServe: the `http.server`-shaped
compatibility classes, the handler-only `eggserve.lowlevel` runtime
substrate, the PyO3 native bridge, the typed stubs, the wheel packaging, and
the Python-visible contracts in `docs/python-api.md` and
`docs/python-http-server-compatibility.md`.

Owns: `crates/eggserve-python/` (Rust bridge, `python/eggserve/*.py`,
`*.pyi`, `py.typed`), the `eggserve` wheel metadata, and the Python half of
the conformance/behavioral parity suites.

Consumes: `eggserve-core` (compatibility umbrella), `eggserve-primitives`,
`eggserve-static` (planning values only), `eggserve-server` (indirectly
through core). Never owns path/filesystem decisions, HTTP framing, or the
H1 runtime — those stay in `eggserve-static` / `eggserve-server`.

Checked by `scripts/check-crate-topology.py` (core usage in the Python crate
is restricted to the documented blocker files) and
`scripts/check-python-release-metadata.py` (three-way wheel version
agreement).

## 2. Work classification

### Invariants

- The Python facade is a *facade*: it must not re-implement static
  planning, path validation, HTTP framing, or response normalization.
- `RequestBody` stays one-shot on the Python side; `read()` and
  `iter_chunks()` are mutually exclusive.
- `#[pyclass(frozen)]` on public Python value types; no `println!`/
  `eprintln!` in bridge code.
- The shipped `.pyi` stubs must describe the runtime that is actually
  built; `py.typed` stays present.
- The wheel version agrees across workspace `Cargo.toml`, the Python crate
  `Cargo.toml`, and `pyproject.toml`.
- Python builds remain HTTP/1.1-only unless a separately scoped plan says
  otherwise; `http2` is not enabled in compatibility builds.
- TLS stays single-identity on the CLI and `HTTPSServer` surfaces.

### Capabilities

- Six-class `http.server` shape: `HTTPServer`, `ThreadingHTTPServer`,
  `HTTPSServer`, `ThreadingHTTPSServer`, `BaseHTTPRequestHandler`,
  `SimpleHTTPRequestHandler`.
- `serve_directory` convenience in `eggserve.subprocess` (re-exported at
  the top level).
- `eggserve.lowlevel`: bounded runtime/service substrate plus the
  experimental `AsyncServer`/`AsyncRequest`/`AsyncResponse`/`AsyncBody`/
  `AsyncTunnel` bridge.
- `eggserve.native` bridge types exposed through the `lowlevel` substrate.

### Infrastructure

- PyO3 bridge modules: `body_bridge`, `request_bridge`, `response_bridge`,
  `sync_handler`, `async_handler`, `tunnel_bridge`, `static_responder`,
  `errors`, `lifecycle`, `runtime`.
- Typed stubs (`_native.pyi`, `lowlevel.pyi`, `server.pyi`, `subprocess.pyi`,
  `__init__.pyi`) and `py.typed`.
- Python conformance/parity suites under `crates/eggserve-python/tests/`.

### Polish

- Packaging smoke tests under `crates/eggserve-python/packaging-tests/`.
- Stub/registry fidelity checks.

## 3. Non-goals

- No in-tree ASGI/WSGI server, lifespan, workers, reload, or router
  (`docs/non-goals.md`).
- No second native runtime: the async bridge reuses the shared native
  runtime and conversion helpers; it must not add a second accept loop or
  duplicate conversion logic.
- No WebSocket framing in-tree; tunnel handoff only.
- No filesystem policy, MIME table, path validation, or response
  normalization inside the bridge.
- No HTTP/2 or HTTP/3 in Python compatibility builds.
- No change to the `docs/non-goals.md` boundary without a same-PR update.

## 4. Current state

The Python surface is the most complete and most heavily tested part of the
repository.

- `crates/eggserve-python/src/` is roughly 6,000 lines of Rust across 13
  files; `crates/eggserve-python/python/eggserve/` holds the Python
  package, stubs, and `py.typed`.
- `crates/eggserve-python/tests/` holds 21 Python test files (roughly 15,500
  lines) plus 5 packaging smoke files; there are no Rust `#[cfg(test)]`
  modules in the crate, which is the correct shape for a PyO3 crate whose
  behavior is exercised from Python.
- Stub fidelity is currently exact: the `_native.pyi` exception set (20
  names) and `pyclass` surface (47 names) match the Rust runtime with no
  missing or extra entries. This must be preserved by any change that adds,
  removes, or renames a bridged type.
- `eggserve.lowlevel` implements the Plan 204 async substrate
  (`AsyncBody`, `AsyncTunnel`, `AsyncTunnelCapability`, `AsyncRequest`,
  `AsyncResponse`, `AsyncServer`) over the shared native runtime;
  `crates/eggserve-python/src/server/async_handler.rs` is an 11-line
  pointer that deliberately owns no conversion logic.
- The bridge is an adapter, not a reimplementation: it consumes
  `eggserve_core::server::config::RuntimeConfig`, `LifecycleState`,
  `validate_static_metadata`, `ServeConfig`, and
  `DEFAULT_MAX_LISTING_ENTRIES`.

Known gaps at the baseline:

- The bridge carries its **own directory-listing renderer**
  (`crates/eggserve-python/src/server/static_responder.rs`) instead of
  using the `eggserve-static` renderer. It reproduces the HTML and the
  escaper but omits the configured response-byte ceiling, and its HTML
  preamble differs from the static authority's. See Milestone 311.
- The Python bridge's listing entry-count bound comes from
  `eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES` rather than from the
  static authority's configurable `max_listing_entries` /
  `max_listing_response_bytes`, so the two surfaces can diverge on both
  bounds.
- The `python-bindings-internal` feature is never enabled in CI or in
  `scripts/verify.sh`, so the Python-only internal code path is never
  clippy-linted. Tracked by the `cli-ops-observability` subsystem, not here.
- Plan 238 (async producer redesign) is deferred; the current async bridge
  is a bounded `asyncio.to_thread` + 16-chunk queue bridge and stays
  experimental.

## 5. Target architecture

The bridge remains a thin adapter over the core compatibility runtime and
the static authority. Every static-serving decision (listing HTML, listing
bounds, MIME, conditional/range planning) is taken once, by
`eggserve-static`, and projected into Python. The Python bridge converts
canonical values into Python objects and nothing more.

## 6. Dependency graph

```text
core compatibility runtime + static authority (closed)
    |
    `--> Milestone 311 (listing renderer consolidation)
```

Milestone 311 has no hard dependency on other open milestones; it depends
on the closed `eggserve-static` authority and on the closed core runtime
projection.

## 7. Milestones

### Milestone 311 — Directory-listing renderer consolidation

Class: invariant

Objective: remove the bridge's second directory-listing renderer so the
Python facade cannot emit a listing that differs from the `eggserve-static`
authority in HTML, escaping, or response-size bounds.

Dependencies: `eggserve-static` listing authority (hard, closed); core
compatibility runtime projection (hard, closed).

Deliverable boundary: one renderer; identical bytes for the same listing
entries; the configured response-byte ceiling enforced on the Python path;
no change to `eggserve-static`'s existing behavior.

User or operator value: Python and Rust clients receive the same listing
bytes and the same size guarantee; the response ceiling cannot be exceeded
through the Python facade.

Exit conditions: byte-identical listing output across surfaces; a Python
test proves a listing that would exceed the byte ceiling is refused rather
than emitted; no duplicate renderer or escaper remains.

## 8. Cross-cutting requirements

### Storage and migration

None; no on-disk state.

### Protocol and compatibility

The Python facade stays HTTP/1.1-only. Listing bytes are a user-visible
contract: any change to the static authority's listing HTML changes the
Python response too, and must be recorded in
`docs/python-http-server-compatibility.md`.

### Security and authorization

Directory listing remains opt-in (`DirectoryListingPolicy`). The listing
byte ceiling and entry ceiling are denial-of-service bounds and must be
enforced on every path that can produce a listing.

### Concurrency, cancellation, and recovery

Unchanged; the bridge keeps the existing bounded queue semantics.

### Observability and audit

Bridge diagnostics go through the runtime ops context; no `println!` in
bridge code.

### Performance and resource use

Listing rendering must not buffer more than the configured ceiling.

### Documentation and operations

`docs/python-api.md` and
`docs/python-http-server-compatibility.md` record any listing-output
change; `architecture/eggserve-python.md` records the ownership boundary.

## 9. Verification strategy

Python test suite (`crates/eggserve-python/tests/`), packaging smoke tests,
the Rust workspace suite through the excluded-crate check, stub/runtime
fidelity comparison, and `./scripts/verify.sh fast` plus the Python lane of
`./scripts/verify.sh full`.

## 10. Risks and decision points

- The static authority's listing renderer is currently crate-private.
  Exposing it is an additive public API change on `eggserve-static` and
  therefore a semver event; the alternative (duplicating the ceiling check
  in the bridge) reintroduces the second authority this milestone removes.
- Listing HTML is user-visible output. Changing it to satisfy a bound
  check is a compatibility change and must not happen silently.

## 11. Completion definition

The Python facade's static behavior is produced by the `eggserve-static`
authority alone; the bridge contains no static-serving decision; stub
fidelity holds; and the Python contract documents match the shipped
behavior.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 311 | ready | `plans/implementation/python-facade/311-directory-listing-renderer-consolidation.md` | — | — |
