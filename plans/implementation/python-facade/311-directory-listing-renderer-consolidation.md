# Python Facade Milestone 311 — Directory-listing renderer consolidation and response-bound restoration

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/python-facade-roadmap.md#milestone-311--directory-listing-renderer-consolidation`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (Python `http.server`-shaped facade; single static authority)
- `plans/001-terminology-and-domain-model.md` (`ListingEntry`, `StaticPolicy`)
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/eggserve-static.md` (listing rendering and budgets)
- `architecture/response-planning.md`
- `docs/python-api.md`
- `docs/python-http-server-compatibility.md`
- `docs/security-policy.md` (directory listing is opt-in)
- `release/plan-225-compatibility-facade-closure.md` (Plan 215 facade authority)

Primary class: invariant

## 1. Objective

Remove the Python bridge's second directory-listing renderer so that a
listing served through the Python facade cannot differ from the same
listing served through the Rust surface, and cannot exceed the response
size bound the Rust surface enforces.

Today `crates/eggserve-python/src/server/static_responder.rs:396-431`
carries its own `directory_listing_bytes` renderer and its own HTML
escaper. The escaper is functionally identical to the static crate's
(`crates/eggserve-static/src/lib.rs:522` `html_escape` — same entity set,
same control-character handling, same `&#x{:X};` form), but:

1. **The response byte bound is not enforced.** The static authority's
   `render_directory_listing` refuses to emit a listing larger than
   `max_response_bytes` (`crates/eggserve-static/src/lib.rs:485-515`); its
   default is 1 MiB (`crates/eggserve-static/src/lib.rs:110`). The Python
   renderer has no such check and bounds only the **entry count** at
   `DEFAULT_MAX_LISTING_ENTRIES` (4096,
   `crates/eggserve-core/src/limits.rs:24`).
2. **The HTML wrapper differs.** The static authority emits

   ```
   <!DOCTYPE html>\n<html>\n<head>\n<meta charset="utf-8">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n
   ```

   while the bridge emits

   ```
   <!DOCTYPE html>\n<html>\n<head><meta charset="utf-8"><title>Directory listing</title></head>\n<body><h1>Directory listing</h1>\n<ul>\n
   ```

   The suffix (`</ul>\n</body>\n</html>\n`) is identical.

This milestone makes the Python facade consume the `eggserve-static`
renderer, restoring the byte bound and making the two surfaces
byte-identical.

## 2. Why this milestone is ready

- The static renderer already exists and already enforces the bound; the
  bridge only needs to call it.
- The escaper equivalence is already proven by inspection and must be
  proven by test as part of this milestone.
- The bridge is an adapter by design
  (`architecture/crate-topology.md:196-202` documents the Python listing
  call), so consuming the authority is a return to the intended shape, not
  a redesign.
- No hard dependency on other open milestones.

## 3. Current implementation evidence

### 3.1 The two renderers

`crates/eggserve-static/src/lib.rs:479-520`:

```rust
fn render_directory_listing(
    entries: &[ListingEntry],
    max_response_bytes: usize,
) -> Result<Vec<u8>, ServiceError> {
    let prefix = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n";
    let suffix = "</ul>\n</body>\n</html>\n";
    if prefix.len().checked_add(suffix.len()).is_none_or(|l| l > max_response_bytes) {
        return Err(ServiceError::internal("directory listing exceeds configured bound"));
    }
    // ... per-entry link, with the same bound re-checked per link ...
}
```

The function is **crate-private** (`fn`, not `pub fn`).

`crates/eggserve-python/src/server/static_responder.rs:396-431`:

```rust
pub(super) fn directory_listing_bytes(entries: &[eggserve_static::ListingEntry]) -> Vec<u8> {
    fn escape(value: &str) -> String { /* identical entity set */ }
    let mut html = String::from("<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\">…");
    for entry in entries { /* ... */ }
    html.push_str("</ul>\n</body>\n</html>\n");
    html.into_bytes()
}
```

The function returns `Vec<u8>` — there is no failure path, hence no bound.

### 3.2 The call site

`crates/eggserve-python/src/server/static_responder.rs:204-220`:

```rust
let entries = dir
    .list(&self.root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
    .map_err(/* ... */)?;
let body = directory_listing_bytes(&entries);
let body_len = body.len();
let mut response = PyResponse::bytes(200, body, None)?;
response.headers.insert("content-type".into(), "text/html; charset=utf-8".into());
```

The entry-count bound comes from the core compatibility crate's constant
rather than from the static authority's configurable
`max_listing_entries` / `max_listing_response_bytes`
(`crates/eggserve-static/src/lib.rs:44-45, 109-110`).

### 3.3 The bound's reachability

`max_listing_response_bytes` is configurable through
`eggserve_static::…::listing_limits(max_entries, max_response_bytes)`
(`crates/eggserve-static/src/lib.rs:70-72`), and the static service uses
it at `crates/eggserve-static/src/lib.rs:331-333`. The Python bridge has no
equivalent knob and no equivalent check.

### 3.4 The observable consequence

With the entry ceiling at 4096 and a filesystem component length that the
platform permits, the Python listing response can exceed the 1 MiB bound
the Rust path enforces. The two surfaces therefore have different
denial-of-service guarantees for the same directory.

### 3.5 The topology position

`architecture/crate-topology.md:196-202` documents the Python crate's use
of the core compatibility crate as restricted to the documented blocker
files, and `scripts/check-crate-topology.py:1696-1719` enforces that
restriction. The listing renderer is **not** a documented exception — it is
an unmarked second implementation.

## 4. Invariants that must not regress

- Directory listing remains **opt-in**. The default policy still refuses
  to render a listing, and nothing in this milestone changes the default.
- The listing is produced by the `eggserve-static` authority. The bridge
  must not render, escape, truncate, or bound listing HTML itself.
- The configured response-byte ceiling must be enforced on **every** path
  that can produce a listing, including the Python path.
- The entry-count ceiling is unchanged (4096 by default) unless the
  configuration says otherwise.
- The listing HTML bytes the Rust surface produces today are unchanged.
- The Python facade's public surface, constructor signatures, and defaults
  are unchanged.
- `eggserve-static` keeps sole ownership of path/filesystem decisions.
- The Python crate's core dependency stays within the gate's documented
  restriction.
- No `println!`/`eprintln!` in bridge code; diagnostics go through the
  runtime ops context.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.

## 5. Scope

### In scope

- Expose the static authority's listing renderer (and, if needed, the
  listing bounds it uses) through a public API on `eggserve-static`, or
  provide an equivalent public listing-render entry point that enforces
  the same bounds.
- Replace the bridge's `directory_listing_bytes` with a call to that
  authority, including the bound enforcement.
- Bind the Python path's listing bounds to the same source of truth as the
  static authority's, so the entry count and the byte ceiling cannot drift
  between the surfaces.
- Decide and record the failure behavior when the bound is exceeded: the
  Python path must fail the request rather than emit an oversized listing,
  and the error must be a clean Python exception rather than a panic.
- Tests proving byte-identical listing output across the Rust and Python
  surfaces, and proving the byte ceiling is enforced on the Python path.
- A topology-gate rule that prevents a second listing renderer from
  reappearing in the Python crate, with a `--self-test` mutation.
- Documentation updates for the listing contract.

### Explicitly out of scope

- No change to the listing HTML format, the escaper, or the static
  authority's existing output.
- No change to the default listing policy or the default bounds.
- No new Python API for configuring the listing bounds, unless the
  existing `StaticPolicy` surface already has a place for it (see §14).
- No change to the `eggserve-static` directory enumeration, the
  `ListingEntry` type, or the link encoding.
- No change to the wheel's version or publication state (Plan 305).
- The lint-policy milestone, the dependency-specification milestone, and
  the cross-platform compile milestone.
- Publication.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `eggserve-static`: gains a public listing-render entry point (additive
  public API) that performs the same rendering and the same bound checks
  as the internal `render_directory_listing`, or the renderer itself is
  made public. The public surface must let a caller supply the bound and
  observe a refusal.
- `eggserve-python`: `static_responder.rs` calls the authority instead of
  rendering locally; the local `escape` helper is removed.
- `eggserve-core`: unchanged, unless the `DEFAULT_MAX_LISTING_ENTRIES`
  constant must be re-pointed at the static authority's default so the two
  cannot drift. A re-point must preserve the value (4096).
- `eggserve-server`, `eggserve-bin`, `eggserve-h3`, `eggnet-tls`,
  `eggserve-primitives`: unchanged.

No new crate. No ownership boundary moves.

### Config and policy

- The Python path must use the same bound values as the static authority.
  The preferred shape is for the Python path to read the bounds from the
  static configuration it already builds, rather than from a separate
  core constant.
- The default bounds are unchanged: 4096 entries, 1 MiB response bytes.
- Directory listing remains disabled by default.

### Protocol and compatibility

- The listing response body bytes become identical to the Rust surface's
  bytes. **This is an observable change** for Python clients that compare
  the listing HTML exactly. It is a correction toward the documented
  authority, and it must be recorded in `docs/migration-guide.md` and in
  `docs/python-http-server-compatibility.md`.
- The `Content-Type: text/html; charset=utf-8` header and the `200` status
  are unchanged.
- No other protocol change.

### Runtime and concurrency

No change. The bridge still renders the listing synchronously on the
request path.

### Frontend or operator surface

- The Python facade's public surface is unchanged.
- The listing failure mode when the byte ceiling is exceeded must be a
  clean, documented Python exception (the bridge's existing error mapping
  convention), not a panic and not an oversized response.

### Security and confinement

- The byte ceiling is a denial-of-service bound. Restoring it on the
  Python path is the security-relevant outcome of this milestone.
- The listing must still be refused unless the policy opts in.
- The Python path must not escape the confined root; the entry list still
  comes from the confined directory listing.

### Documentation and static guards

- `docs/python-http-server-compatibility.md` — the listing body contract.
- `docs/python-api.md` — if the listing bounds become reachable from
  Python.
- `architecture/eggserve-static.md` — the public listing-render API.
- `architecture/eggserve-python.md` — the bridge's dependency on the
  static authority for listing.
- `scripts/check-crate-topology.py` — the rule preventing a second
  renderer.

## 7. Ordered work packages

### Work package A — Capture the baseline listing output

Capture the current Rust-surface listing bytes and the current
Python-surface listing bytes for the same set of entries, including a name
that exercises each escape branch (`&`, `<`, `>`, `"`, `'`, and a control
character), and a name with a non-UTF-8 byte sequence.

Record both captures. The Rust capture is the target; the Python capture
is the current state to be corrected.

Acceptance evidence: both captures recorded, with the exact difference
(the HTML wrapper) named.

### Work package B — Expose the static listing renderer

Add a public entry point on `eggserve-static` that renders a listing with a
supplied byte bound and returns a refusal when the bound is exceeded.

Requirements:

- the rendered bytes are exactly the current Rust-surface bytes;
- the bound check is the same check the internal renderer performs,
  including the prefix/suffix pre-check and the per-link re-check;
- the refusal is a typed error the bridge can map to a Python exception;
- the API is additive and does not change the static service's behavior.

Acceptance evidence: a static-crate test asserts the public renderer
produces the current bytes and refuses an oversized listing.

### Work package C — Replace the bridge renderer

Delete `directory_listing_bytes` and its local `escape` helper from
`crates/eggserve-python/src/server/static_responder.rs` and call the
authority's renderer instead.

Bind the Python path's bounds to the same source of truth as the static
authority's configuration.

Map the refusal to a clean Python exception using the bridge's existing
error-mapping convention.

Acceptance evidence: the bridge contains no HTML rendering or escaping
code; the Python listing response is byte-identical to the Rust surface's.

### Work package D — Enforce the bound and prove it

Add a test that constructs a directory whose listing exceeds the byte
ceiling and asserts:

- the Python path refuses it with the documented exception;
- the Rust path refuses it with the same condition;
- neither path emits an oversized body.

Acceptance evidence: both paths refuse; no oversized body is observed.

### Work package E — Guard against a second renderer

Add a topology-gate rule that fails if the Python crate contains its own
listing HTML rendering or HTML escaping, and include a `--self-test`
mutation.

The rule should look for the concrete markers of a second renderer (a
local escaper, the listing HTML prefix literal, or the `</ul>` suffix) so
that it is specific rather than a blanket ban.

Acceptance evidence: the gate passes; the `--self-test` mutation proves
the rule is wired.

### Work package F — Documentation

Update `docs/python-http-server-compatibility.md`,
`docs/python-api.md`, `architecture/eggserve-static.md`, and
`architecture/eggserve-python.md` to describe one listing contract with
one set of bounds.

Acceptance evidence: no document describes a Python-specific listing
format or a Python-specific bound.

## 8. Failure, cancellation, restart, and contention semantics

- A listing that exceeds the byte ceiling fails the request cleanly. The
  bridge must not truncate the listing, must not emit a partial listing,
  and must not panic.
- The refusal must not leave the response half-written. The bridge's
  existing error path handles it, and the test must observe a well-formed
  failure.
- No change to cancellation or shutdown behavior.
- The renderer must not buffer beyond the bound. The static authority's
  per-link bound re-check already guarantees this; the public API must not
  relax it.

## 9. Compatibility and migration

- The Python listing body bytes change to match the Rust surface's bytes.
  A Python client that string-matched the old HTML wrapper would see a
  difference. Record this in `docs/migration-guide.md`.
- The listing response size ceiling is restored on the Python path, so a
  listing that previously succeeded may now fail. Record this as a
  deliberate correction.
- `eggserve-static` gains additive public API (patch per the 291
  precedent, unless the API shape warrants a minor).
- No Python public API change and no default change.

## 10. Required tests

### Focused unit tests

- The static crate's public renderer produces the expected bytes for a
  representative entry set.
- The renderer refuses an oversized listing (prefix/suffix pre-check and
  per-link re-check).
- The escape branches produce the documented entity forms.

### Integration tests

- The Python facade serves a listing whose bytes equal the Rust surface's
  bytes for the same directory.
- The Python facade refuses an oversized listing with the documented
  exception.
- The Python facade still refuses a listing when the policy does not opt
  in.
- The `Content-Type` header and the `200` status are unchanged.

### Restart and recovery tests

None required.

### Contention and cancellation tests

None required.

### Security and negative tests

- A listing with a name containing HTML metacharacters is escaped
  identically on both surfaces.
- A name with a non-UTF-8 byte sequence is escaped identically on both
  surfaces (the link target remains encoded from raw bytes).
- The byte ceiling cannot be exceeded through the Python path.

### Migration and compatibility tests

- The pre-change Rust-surface bytes are unchanged by this milestone.
- The Python-surface bytes change to the Rust-surface bytes, and the
  change is recorded in the migration guide.

## 11. Required verification commands

```bash
cargo test -p eggserve-static --lib
cargo test -p eggserve-core --lib
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
python3 -m pytest crates/eggserve-python/tests -q
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --locked --lib -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

The Python test run requires the built extension; use the repository's
existing wheel/test procedure (see `scripts/test-python-wheel.sh`) if a
direct `pytest` run cannot load the native module.

## 12. Documentation updates

- `docs/python-http-server-compatibility.md` — the listing body contract
  and the byte ceiling.
- `docs/python-api.md` — listing bounds, if they become reachable.
- `docs/migration-guide.md` — the listing byte change and the restored
  ceiling.
- `architecture/eggserve-static.md` — the public listing-render API.
- `architecture/eggserve-python.md` — the bridge's static-authority
  dependency for listing.
- `plans/subsystems/python-facade-roadmap.md` — Milestone 311 status.
- `plans/registry.md` — the milestone row.
- `plans/closure/python-facade/311-directory-listing-renderer-consolidation.md`
  (new).

## 13. Acceptance criteria

- The Python bridge contains no listing HTML rendering or escaping code.
- A listing served through the Python facade is byte-identical to the same
  listing served through the Rust surface.
- The configured byte ceiling is enforced on the Python path, and an
  oversized listing is refused rather than emitted.
- The entry-count ceiling is unchanged and read from the same source of
  truth as the static authority's.
- The topology gate fails if a second listing renderer reappears in the
  Python crate, and the `--self-test` mutation proves the rule is wired.
- The listing opt-in default is unchanged.
- The change is recorded in the migration guide as an observable
  correction, not silently applied.

## 14. Stop conditions

Stop and report rather than improvise if:

- exposing the static renderer publicly would require a non-additive change
  to `eggserve-static`;
- making the two surfaces byte-identical would require changing the
  **Rust** surface's listing bytes — that would be a different (and
  user-visible) decision, and needs its own approval;
- the Python path cannot obtain the configured bounds from the static
  configuration it already builds, and threading them would require
  changing the Python facade's public constructor signatures — report the
  gap rather than widening the public API;
- binding the byte ceiling to the Python path would break an existing
  Python test that asserts a large listing succeeds — that test is
  asserting the defect; update it deliberately and record the change;
- the maintainer wants a new Python option to configure the listing bounds
  — that is a Python API addition and belongs in its own plan.

## 15. Closure evidence required

Create `plans/closure/python-facade/311-directory-listing-renderer-consolidation.md`
containing:

- the baseline Rust and Python listing captures and the post-change
  comparison;
- the escape-branch equivalence evidence;
- the oversized-listing refusal evidence for both paths;
- the public API added to `eggserve-static` and its semver classification;
- the gate rule text and the `--self-test` mutation result;
- the documentation diffs, including the migration-guide entry;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The Python facade is an adapter. The temptation is to "fix" the byte
bound by adding a check in the bridge — that keeps two renderers and two
bound policies. The whole point is that `eggserve-static` decides what a
listing is and how big it may be. Also: the listing bytes *will* change
for Python clients, and that change must be recorded rather than shipped
as an invisible difference.
