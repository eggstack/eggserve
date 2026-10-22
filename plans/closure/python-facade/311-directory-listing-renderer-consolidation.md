# Python Facade Milestone 311 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/python-facade/311-directory-listing-renderer-consolidation.md`

Source subsystem roadmap:

- `plans/subsystems/python-facade-roadmap.md`

Repository baseline reviewed: `a7e2f22` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Uncommitted working-tree implementation at closure time (the maintainer owns
  the commit and the release line).

## 1. Executive finding

The Python bridge no longer carries a directory-listing renderer. It
enumerates entries and hands them to `eggserve-static`, which owns the HTML
shape, the HTML escaping, the entry-count bound, **and** the response-byte
ceiling.

Two defects are fixed:

1. **The 1 MiB response-byte ceiling now applies to the Python path.** The
   bridge previously bounded only the *entry count* and returned `Vec<u8>`
   with no failure path, so a Python-served listing could exceed the ceiling
   the Rust surface enforced. The two surfaces enforced different
   denial-of-service guarantees for the same directory.
2. **The two surfaces now emit byte-identical HTML.** The bridge's wrapper
   differed from the authority's in whitespace and element layout, so a
   directory listing served through Python was not the same bytes as the same
   directory served through Rust.

The Python facade's public surface, constructor signatures, and defaults are
unchanged. Directory listing remains opt-in.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| The bridge owns no renderer | met | `directory_listing_bytes` and the local `fn escape` helper are deleted from `crates/eggserve-python/src/server/static_responder.rs`; no `fn escape`, `html_escape`, `<!DOCTYPE html>`, `Directory listing`, `</ul>`, or `directory_listing_bytes` token remains anywhere under `crates/eggserve-python/src/` |
| The bridge calls the authority | met | `crates/eggserve-python/src/server/static_responder.rs:220` — `eggserve_static::render_directory_listing(&entries, eggserve_static::DEFAULT_MAX_LISTING_RESPONSE_BYTES)` |
| The renderer is publicly reachable | met | `crates/eggserve-static/src/lib.rs:509` — `pub fn render_directory_listing(entries: &[ListingEntry], max_response_bytes: usize) -> Result<Vec<u8>, ServiceError>` |
| The byte ceiling is enforced on the Python path | met | The authority's prefix/suffix pre-check and per-link re-check both apply; the bridge maps the refusal (§3.4) |
| The entry bound has one source of truth | met | `crates/eggserve-static/src/lib.rs:45` — `pub const DEFAULT_MAX_LISTING_ENTRIES: usize = 4096;`; `crates/eggserve-core/src/limits.rs:29` — `pub use eggserve_static::DEFAULT_MAX_LISTING_ENTRIES;`; the Python bridge reads `eggserve_static::DEFAULT_MAX_LISTING_ENTRIES` directly |
| The entry-count value is unchanged | met | 4096, asserted by a test in `crates/eggserve-core/src/limits.rs` |
| The response-byte default is unchanged | met | `crates/eggserve-static/src/lib.rs:51` — `pub const DEFAULT_MAX_LISTING_RESPONSE_BYTES: usize = 1024 * 1024;` |
| The Rust surface's listing bytes are unchanged | met | The rendered format is untouched — only the visibility of the renderer and the constants changed (§3.2) |
| The refusal is a clean, documented Python exception | met | `crates/eggserve-python/src/server/errors.rs:68` — `listing_error_to_pyerr` maps the `ServiceError` to `pyo3::exceptions::PyRuntimeError` with the message `directory listing failed: …` |
| No panic, no truncation, no partial body, no oversized response | met | The authority returns `Err` before emitting anything; the bridge propagates the error and produces no response |
| The Python facade's public surface, signatures, and defaults are unchanged | met | No signature or default changed; only the internals of the listing branch |
| Directory listing remains opt-in | met | The branch is still gated on `policy::DirectoryListingPolicy::Enabled`; the default policy is unchanged |
| `Content-Type` and the `200` status are unchanged | met | `text/html; charset=utf-8`, `nosniff`, the CSP and `no-referrer` headers, and status `200` are unchanged |
| Machine-checked authority | met | `scripts/check-crate-topology.py::check_plan311_listing_renderer_authority` + 6 `--self-test` mutations |

## 3. Production implementation evidence

### 3.1 The change

| File | Change |
|---|---|
| `crates/eggserve-static/src/lib.rs` | `render_directory_listing` is now `pub`; `DEFAULT_MAX_LISTING_ENTRIES` (4096) and `DEFAULT_MAX_LISTING_RESPONSE_BYTES` (1 MiB) are now public constants; `html_escape` stays crate-private |
| `crates/eggserve-core/src/limits.rs` | `DEFAULT_MAX_LISTING_ENTRIES` is now `pub use eggserve_static::DEFAULT_MAX_LISTING_ENTRIES;` — a re-export, not a redefinition; `max_listing_response_bytes` reads `eggserve_static::DEFAULT_MAX_LISTING_RESPONSE_BYTES` |
| `crates/eggserve-python/src/server/static_responder.rs` | `directory_listing_bytes` and the local `escape` helper deleted; the branch calls `eggserve_static::render_directory_listing(...)` and maps the refusal |
| `crates/eggserve-python/src/server/errors.rs` | New `listing_error_to_pyerr` mapping the `ServiceError` to `PyRuntimeError` |
| `crates/eggserve-static/tests/listing_renderer.rs` *(new)* | The renderer's byte contract and its two bound checks |
| `crates/eggserve-python/tests/test_directory_listing_authority.py` *(new)* | The facade's byte-identity with the Rust surface, the oversized-listing refusal, and the opt-in behavior |

### 3.2 The HTML difference, precisely

**Before** — the Rust surface (`eggserve-static`):

```
<!DOCTYPE html>\n<html>\n<head>\n<meta charset="utf-8">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n
```

**Before** — the Python bridge (a second renderer):

```
<!DOCTYPE html>\n<html>\n<head><meta charset="utf-8"><title>Directory listing</title></head>\n<body><h1>Directory listing</h1><ul>\n
```

The closing wrapper was already identical (`</ul>\n</body>\n</html>\n`), and the
per-entry `<li>` markup was already identical — the same escape table
(`& < > " '`, control characters to `&#x1B;`) and the same percent-encoded
hrefs. The difference was the wrapper: newlines after `<head>`/`<body>`/`<h1>`
and before `<ul>`.

**The exact difference is five `\n` bytes.** Measured on a three-entry
directory: **263 → 268 bytes**. The Rust surface's bytes are unchanged
(before == after); only the Python surface changed, and it changed to the
Rust surface's bytes.

**After** — both surfaces emit the Rust surface's bytes, because there is now
only one renderer.

### 3.3 The bounds

| Bound | Value | Owner | Python path |
|---|---|---|---|
| Entry count | 4096 | `eggserve_static::DEFAULT_MAX_LISTING_ENTRIES` | `dir.list(&self.root, eggserve_static::DEFAULT_MAX_LISTING_ENTRIES)` |
| Response bytes | 1 MiB | `eggserve_static::DEFAULT_MAX_LISTING_RESPONSE_BYTES` | passed to `render_directory_listing` |

The authority enforces the byte ceiling in two places: a prefix/suffix
pre-check before any entry is rendered, and a per-link re-check as the
listing grows. Both are now reachable from the Python path, which is the
whole point of the milestone.

### 3.4 The refusal path

`render_directory_listing` returns
`Err(ServiceError::internal("directory listing exceeds configured bound"))`.
The bridge maps it through `listing_error_to_pyerr` to
`pyo3::exceptions::PyRuntimeError` with the message
`directory listing failed: directory listing exceeds configured bound`. No
response object is constructed, so no partial or oversized body can escape.

The builtin `RuntimeError` was chosen over a new `_native` exception class on
purpose: a new class would need a matching stub in `_native.pyi`, which would
change the facade's public surface. At the HTTP layer the exception becomes
the facade's standard generic 500 (`python/eggserve/server.py:253-265`) and
the server keeps serving.

## 4. Verification executed

### Commands run (parent-verified)

| Command | Outcome |
|---|---|
| `cargo fmt --all -- --check` | PASS (clean) |
| `cargo clippy -p eggserve-static --lib --tests -- -D warnings` | PASS |
| `cargo clippy -p eggserve-core --lib --tests -- -D warnings` | PASS |
| `cargo test -p eggserve-static --lib` | PASS — 366 passed, 0 failed, 3 ignored |
| `cargo test -p eggserve-static --test listing_renderer` | PASS — 7 passed, 0 failed |
| `cargo test -p eggserve-core --lib` | PASS — 161 passed, 0 failed |
| `cargo test -p eggserve-core --test corpus_replay` | PASS — 11 passed, 0 failed |
| `cargo check --manifest-path fuzz/Cargo.toml --bins --locked` | PASS |
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | PASS |
| `bash scripts/test-python-wheel.sh` | PASS — 851 tests, all OK (the six new ones included) |
| `cargo test -p eggserve-core` (all targets) | PASS — all green |
| `python3 scripts/check-crate-topology.py` | PASS (exit 0) |
| `python3 scripts/check-crate-topology.py --self-test` | PASS — "self-test: 67 checks passed" (incl. `live-plan311`) |

**One lane is red, and it is not this milestone's.** `cargo clippy
--manifest-path crates/eggserve-python/Cargo.toml --locked --lib -- -D
warnings` reports six lints in the excluded Python crate:
`server/body_bridge.rs:86` (`needless_borrow`),
`server/static_responder.rs:46` (`too_many_arguments`, the untouched
`respond` signature), `server/sync_handler.rs:514` (`if_same_then_else`),
`server/tunnel_bridge.rs:241` (`type_complexity`), `lib.rs:1271`
(`clone_on_copy`), `lib.rs:1535` (`too_many_arguments`). Each was verified
byte-identical at `HEAD` (`git show HEAD:<path>`) and none is in code this
milestone wrote. They do **not** affect CI: the excluded Python crate is not
a workspace member, and the `python` CI job runs
`bash scripts/test-python-wheel.sh`, not a clippy lane. They are reported for
Milestone 313 (lint policy enforcement), which is where a
`python-bindings-internal` clippy lane would first meet them.

### Test inventory

`crates/eggserve-static/tests/listing_renderer.rs` (7 tests):
`public_renderer_emits_the_canonical_listing_bytes`,
`renderer_refuses_when_the_wrapper_does_not_fit`,
`renderer_refuses_when_a_link_would_exceed_the_ceiling`,
`renderer_refuses_instead_of_truncating_a_long_listing`,
`default_listing_bounds_are_the_documented_values`,
`static_service_emits_the_same_bytes_as_the_public_renderer`,
`static_service_refuses_an_oversized_listing`.

`crates/eggserve-python/tests/test_directory_listing_authority.py` —
`ListingAuthorityTests` (the byte-identity and bound contract of the
authority) and `ListingFacadeTests` (the facade's byte-identity with the
Rust surface, the oversized-listing refusal, the opt-in behavior, and the
unchanged `Content-Type` and status).

## 5. Invariant review

| Invariant | Result |
|---|---|
| Directory listing remains opt-in; the default policy is unchanged | Held. The branch is still gated on `DirectoryListingPolicy::Enabled`. |
| The listing is produced by the `eggserve-static` authority; the bridge never renders, escapes, truncates, or bounds listing HTML itself | Held. The bridge's only role is enumeration and delegation. |
| The response-byte ceiling is enforced on every path that can produce a listing | Held — and this is what the milestone fixes. |
| The entry-count ceiling is unchanged (4096) and read from the same source of truth | Held. |
| The listing HTML bytes the Rust surface produces today are unchanged | Held. The renderer body is untouched. |
| The Python facade's public surface, constructor signatures, and defaults are unchanged | Held. |
| `eggserve-static` keeps sole ownership of path/filesystem decisions | Held. |
| No `println!`/`eprintln!` in bridge code | Held. |
| Directory enumeration and the `ListingEntry` type are unchanged | Held. |
| The link encoding is unchanged | Held — the authority's `href` percent-encoding is untouched. |

## 6. Failure and recovery review

- A listing that exceeds the byte ceiling now produces a clean Python
  exception on the Python path. Previously the Python path would have emitted
  the oversized listing. The failure is total, not partial: the authority
  returns `Err` before constructing a response, so no truncated listing can
  be served.
- The per-entry ceiling check means the failure can also occur part-way
  through rendering; the accumulated HTML is discarded with the error, so a
  partial listing never escapes.
- The two bounds are independent: the entry count (4096) is enforced during
  enumeration, the byte ceiling during rendering. A directory with many small
  entries can hit either limit.

## 7. Migration and compatibility review

- The Python facade's public surface is unchanged. No caller-visible migration
  is required.
- One **behavior** change is observable: a Python-served directory listing
  now has the same bytes as a Rust-served one, and an oversized listing
  raises an exception instead of being served. Anyone asserting on the old
  bridge's exact HTML would see the difference; the new bytes are the ones
  the Rust surface has always produced.
- `eggserve-static` gains an additive public API (`render_directory_listing`
  plus two constants) — a **patch** release event per the Plan 291 precedent.
- The compatibility `eggserve-core` `RuntimeConfig` is unchanged; its
  `DEFAULT_MAX_LISTING_ENTRIES` constant keeps its value and its type, and
  is now a re-export rather than a local definition.

## 8. Security review

- **The response-byte ceiling now covers the Python path.** This is the
  substantive security effect of the milestone: a Python-hosted static server
  could previously serve an arbitrarily large directory listing, bounded only
  by the entry count, while the Rust surface refused anything over 1 MiB. Two
  surfaces of the same product enforced different limits; they now enforce
  the same one.
- The HTML escaping is now single-sourced. The bridge's local `escape`
  helper was a second implementation of HTML escaping; a divergence between
  the two would have been an injection surface. There is now one escaper.
- The listing output is unchanged in content: the same escaped display names
  and the same percent-encoded link targets. The change is the removal of a
  redundant copy, not a relaxation.
- No new dependency, no new `unsafe`, no change to the confinement boundary.
- Directory traversal, dotfile policy, and symlink policy are untouched: the
  bridge still enumerates through `SecureRoot`.

## 9. Documentation and operations

| Surface | Change |
|---|---|
| `docs/python-http-server-compatibility.md` | The listing row now describes the single renderer and the shared byte ceiling. |
| `docs/python-api.md` | The listing behavior and the refusal exception are documented. |
| `architecture/eggserve-static.md` | The static authority's ownership of the listing renderer and both bounds. |
| `architecture/eggserve-python.md` | The bridge's role reduced to enumeration and delegation. |
| `docs/migration-guide.md` | Plan 311 section (parent-owned file, updated by the parent). |
| `scripts/check-crate-topology.py` | New rule `check_plan311_listing_renderer_authority` plus 6 `--self-test` mutations and a live-tree check. |

## 10. Unresolved findings

1. **Six pre-existing clippy lints in the excluded Python crate**
   (§4). Not introduced by this milestone and not reachable from CI today;
   they belong to Milestone 313's lint-policy scope.
2. **The listing byte ceiling is not configurable from Python.** Per the
   plan's stop conditions, the bridge uses the authority's default
   (1 MiB). A directory whose rendered listing exceeds it is a hard generic
   500; there is no Python-side knob to raise or lower it. Changing that
   would require a configuration surface the plan deliberately excluded.
3. `crates/eggserve-python/Cargo.toml`'s dependency comment still claimed the
   compatibility core supplied the bridge's "static listing budgets". That
   stopped being true with this milestone; the parent corrected the comment.

## 11. Roadmap disposition

The python-facade roadmap's Milestone 311 ("Directory-listing renderer
consolidation") is **closed**. The roadmap's "one renderer, one bound"
invariant is now true across both surfaces. Publication is a separate
milestone; `eggserve-static`'s additive public surface is a patch release
event when it ships.

## 12. Registry updates

- `plans/registry.md` — Milestone 311 moved to `closed` with a pointer to this
  closure record.
- `plans/subsystems/python-facade-roadmap.md` — Milestone 311 status and
  closure-record column updated.
- A new topology-gate rule `check_plan311_listing_renderer_authority` was
  added to `scripts/check-crate-topology.py`. It fails if the bridge grows a
  second renderer (`fn escape(`, `html_escape`, `<!DOCTYPE html>`,
  `Directory listing`, `</ul>`, `directory_listing_bytes`), if the bridge
  stops delegating to the authority, if the bridge hard-codes either bound
  instead of reading `eggserve_static::DEFAULT_MAX_LISTING_ENTRIES` and
  `eggserve_static::DEFAULT_MAX_LISTING_RESPONSE_BYTES`, if the
  compatibility crate redefines the entry bound instead of re-exporting it,
  or if the authority's escaper becomes public.
