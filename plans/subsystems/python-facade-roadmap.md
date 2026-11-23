# Python Facade Roadmap

Status: active (311, 322, and 326 closed; 327 closed with the tier decision recorded)

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
  preamble differs from the static authority's. **Closed by Milestone 311**:
  the bridge now calls `eggserve_static::render_directory_listing`, the two
  surfaces emit byte-identical HTML, and the 1 MiB ceiling applies to both.
- The Python bridge's listing entry-count bound comes from
  `eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES` rather than from the
  static authority's configurable `max_listing_entries` /
  `max_listing_response_bytes`, so the two surfaces can diverge on both
  bounds. **Closed by Milestone 311**: both bounds now come from
  `eggserve-static` (`DEFAULT_MAX_LISTING_ENTRIES` and
  `DEFAULT_MAX_LISTING_RESPONSE_BYTES`), and the compatibility constant is a
  re-export of the authority's.
- The `python-bindings-internal` feature is never enabled in CI or in
  `scripts/verify.sh`, so the Python-only internal code path is never
  clippy-linted. Tracked by the `cli-ops-observability` subsystem, not here.
- Plan 238 (async producer redesign) is deferred; the current async bridge
  is a bounded `asyncio.to_thread` + 16-chunk queue bridge and stays
  experimental.
- The extension declares free-threading support implicitly: PyO3 >= 0.28
  emits `Py_MOD_GIL_NOT_USED` for a `#[pymodule]` without `gil_used`, so a
  source build on free-threaded CPython 3.14 already runs GIL-free while the
  support contract says free-threaded is unsupported. **Tracked by Milestone
  326**: the declaration becomes explicit and the 3.14t slice is qualified as
  a Beta wheel family.

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
    +--> Milestone 311 (listing renderer consolidation)
    +--> Milestone 322 (streamed-response producer bounds)
    +--> Milestone 326 (free-threaded CPython 3.14t, closed)
    `--> Milestone 327 (qualification evidence + tier decision, closed)
```

Milestone 311 has no hard dependency on other open milestones; it depends
on the closed `eggserve-static` authority and on the closed core runtime
projection. Milestone 322 is an independent post-audit corrective over the
same closed bridge; it does not reopen the static-listing authority.
Milestone 326 depends on the closed bridge and Plan 322, and supersedes only
the free-threaded slice of the legacy Plan 267 record. Milestone 327 depends on
Plan 326's implementation and takes over its two named closure conditions plus
the Beta/Stable decision; it changes no product behavior.

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

### Milestone 322 — Streamed-response producer lifecycle and admission bounds

Class: invariant

Objective: make every streamed-response producer observe response and
connection teardown, and bound concurrent producer threads with an explicit
admission budget, so a handler that returns a blocking or endless iterator
cannot accumulate detached threads per request.

Dependencies: Plan 166 low-level sync substrate (hard, closed); Plan 254
async-streaming parity as the interface precedent (interface, closed).
Independent of Milestone 311: this milestone touches the sync body bridge,
not the listing renderer.

Deliverable boundary: a closed-receiver check before each pull and on empty
yields; a producer admission permit held for the thread's lifetime; a
sanitized 503 on budget exhaustion before response start; documentation of
the blocking-`__next__` limitation. No attempt to kill Python code, no
change to the 16-chunk bridge or backpressure, no async-producer change.

User or operator value: a client can no longer accumulate an unbounded
number of producer threads by repeatedly requesting a route whose iterator
never terminates; a disconnecting client releases its producer at the next
observation point.

Exit conditions: a non-terminating `b""` iterator stops advancing after the
client disconnects; concurrent producers never exceed the budget and
exhaustion is a 503, not a hang; normal streaming, trailers, HEAD/body
suppression, and truncation behavior are unchanged; the residual blocking
limitation is documented.

### Milestone 326 — Free-threaded CPython 3.14t staged support

Class: capability

Objective: publish `cp314-cp314t` wheels for free-threaded CPython 3.14 on
Linux x86_64 glibc, macOS arm64, and Windows x86_64, and record the support
level as Beta until the promotion gate, without changing the GIL
`cp311-abi3` contract.

Dependencies: PyO3 free-threading declaration semantics and the resolved
0.29.3 (hard, closed); maturin 1.14.1 pipeline pin (hard, closed); Plan 267
Track A (interface, closed as DEFERRED; superseded for this slice by this
milestone); Plan 322 producer bounds (hard, closed).

Deliverable boundary: one additive wheel family; an explicit
`#[pymodule(gil_used = false)]` declaration; a free-threaded CI lane;
family-aware wheel-matrix/release validators; free-threaded concurrency
tests; documentation and classifier. No Rust-crate API change, no HTTP,
TLS, or security change, no `abi3t`, no 3.13t/3.15t, and no platform outside
the three targets.

User or operator value: 3.14t users get an installable binary wheel instead
of an unqualified source build, and the documented support level matches the
evidence.

Exit conditions: the family builds and installs on all three targets; the
suite and the new concurrency tests pass with the GIL disabled; the GIL
family is unchanged; the validators enforce exactly the declared family; the
docs state Beta for the three targets only.

### Milestone 327 — Free-threaded 3.14t qualification evidence and tier decision

Class: infrastructure

Objective: produce the two conditions named by Plan 326's conditional closure
(a green PR CI run including the free-threaded lane, and a
`publish_target: none` three-target release aggregate), close Plan 326, then
make one recorded Beta/Stable decision using Plan 326's promotion gate and a
bounded contention soak as the TSan substitute.

Dependencies: Plan 326 implementation (hard, closed); repository
write access and the existing CI/release workflows (operational); owner
confirmation of the aggregate-as-release reading for promotion (interface).
No publication is in scope.

Deliverable boundary: commit/PR/CI evidence, one release-workflow aggregate with
the three `-ft` wheels, the Plan 326 closure update, a stdlib-only soak script
and three records, the TSan assessment, and either the Stable classifier/doc
change or a recorded retain-Beta decision. No behavior, API, dependency,
platform, or publication change.

Exit conditions: CI green with `python-free-threaded`; aggregate green with the
exact 13 platform/family pairs; Plan 326 closed with both run URLs; soak
self-test plus three zero-error records; the promotion gate evaluated and a
decision recorded.

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

GIL-enabled behavior is unchanged: callbacks and stream producers are bounded
by `max_python_callbacks` and the Plan 322 producer budget. On a
free-threaded build, up to that many callbacks and producers can execute
Python code in parallel; the bridge's Rust state stays behind `Mutex` and
atomics, and handler/iterable thread-safety is the caller's responsibility
(documented, with `max_python_callbacks=1` as the serialized mode). Timeouts
still cannot interrupt Python code. No new cancellation or shutdown
semantics.

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
`./scripts/verify.sh full`. The free-threaded 3.14t lane is separate: it
compiles the excluded crate against a `-t` interpreter (`Py_GIL_DISABLED`),
builds and installs a `cp314-cp314t` wheel, asserts the GIL stays disabled,
and runs the suite plus `test_free_threaded.py`.

## 10. Risks and decision points

- The static authority's listing renderer is currently crate-private.
  Exposing it is an additive public API change on `eggserve-static` and
  therefore a semver event; the alternative (duplicating the ceiling check
  in the bridge) reintroduces the second authority this milestone removes.
- Listing HTML is user-visible output. Changing it to satisfy a bound
  check is a compatibility change and must not happen silently.
- Free-threaded handlers execute in parallel, which is a user-visible
  semantic change for handlers that share mutable Python state; the Beta
  tier, the documentation, and `max_python_callbacks=1` are the mitigation.
- Routine CI has no TSan or adversarial soak; the promotion gate names that
  evidence explicitly.
- Free-threaded interpreters are not guaranteed in every release lane
  (ARMv7, Windows arm64, musl), so the family starts on three targets only.

## 11. Completion definition

The Python facade's static behavior is produced by the `eggserve-static`
authority alone; the bridge contains no static-serving decision; stub
fidelity holds; and the Python contract documents match the shipped
behavior.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 311 | closed | `plans/implementation/python-facade/311-directory-listing-renderer-consolidation.md` | `plans/closure/python-facade/311-directory-listing-renderer-consolidation.md` | One listing renderer: the bridge delegates to `eggserve-static`, so both surfaces emit byte-identical HTML and enforce the same 1 MiB response-byte ceiling. Gate rule `check_plan311_listing_renderer_authority`. Reported follow-up: six pre-existing clippy lints in the excluded Python crate, which belong to Milestone 313's scope. |
| 322 | closed | `plans/implementation/python-facade/322-streamed-response-producer-lifecycle-bounds.md` | `plans/closure/python-facade/322-streamed-response-producer-lifecycle-bounds.md` | `Response.stream` spawns a detached producer thread per response. The empty-yield branch never checks the closed receiver, and the callback permit ends before the producer runs, so a blocking/endless iterator can accumulate threads after disconnect/timeout. The plan checks closure before each pull, bounds producers with an admission permit, and documents the non-interruptible `__next__` limitation. |
| 326 | closed | `plans/implementation/python-facade/326-free-threaded-cpython-314t-staged-support.md` | `plans/closure/python-facade/326-free-threaded-cpython-314t-staged-support.md` | Beta `cp314-cp314t` family on Linux x86_64 glibc, macOS arm64, and Windows x86_64; family-aware matrix/validators/release lanes; free-threaded CI lane and concurrency tests. Closed with both named conditions observed: CI run 45277923657 (`python-free-threaded` green, 861 tests on CPython 3.14.10t, GIL stays disabled) and release run 45283657891 (exact 13-pair aggregate with the three `-ft` wheels; 5/5 ABI-proof lanes). |
| 327 | closed | `plans/implementation/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md` | `plans/closure/python-facade/327-free-threaded-314t-qualification-and-tier-decision.md` | Produced both Plan 326 conditions (CI run 45277923657; release run 45283657891, exact 13-pair aggregate), closed Plan 326, added `scripts/soak-python-free-threaded.py` with three zero-error records as the TSan substitute, and evaluated the promotion gate: all four criteria met, family retained at Beta pending the owner's confirmation of the aggregate-as-release reading. No product behavior or publication change. |
