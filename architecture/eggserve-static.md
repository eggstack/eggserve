# eggserve-static

`eggserve-static` is the sole static/path/filesystem authority (Plans 214,
219, 224-NO-GO, 245). It owns path parsing, secure-root resolution,
descriptor/handle-relative confinement, MIME selection, pure response
planning glue, and `StaticService` request-to-response rendering over
`eggserve-server::Service`. `eggserve-core` keeps facades only
(`src/fs`, `src/path`, `src/mime.rs` deleted); security fixes land once
(authority fixture:
`crates/eggserve-core/tests/static_authority_conformance.rs`).

> Guard: `scripts/check-crate-topology.py`. No `eggserve-capfs` crate
> (Plan 224 NO-GO). No pathname check-then-open fallback.

## Manifest

- Deps (`Cargo.toml`): `eggserve-primitives`, `eggserve-server`,
  `httpdate`, `phf{macros}`; Unix-only `rustix{fs,net}`. Feature:
  `python-bindings-internal` (capability bridge only).
- Modules (`src/`): `path/` (public), `fs/` + `secure_root.rs` + `mime.rs` (private, no root re-export)
  + `planner.rs` (crate-private, re-exported at root per `lib.rs`).

## Module inventory

| Area | Contents |
|------|----------|
| `path/` (`mod`/`components`/`decode`/`platform`/`policy`/`rejected`) | `ConfinedPath::{parse, from_path_component, as_str, components, path_policy}`, `PathPolicy{dotfiles, reject_backslash}`, parse-level `DotfilePolicy::{Denied, Allow}`, 17-variant `PathRejection` |
| `secure_root.rs` + `fs/{mod,unix,windows}` | `SecureRoot::{new, policy, root_path, resolve, resolve_uri}`, `resolve_and_plan`, `ResolvedResource::{File, Directory, NotFound, Denied, IoError}`, `ResourceDeniedReason`, `ResolvedFile::{len, modified, metadata, content_type, plan_response, into_body, into_range_body}` (+ gated `into_std_file`/`into_parts`/`from_parts`), `ResolvedDirectory::{components, list, resolve_child}`; internal `PinnedRoot`/`RootGuard`; Unix `statat` + `openat(O_NOFOLLOW)`, Windows handle-relative opens |
| `mime.rs` | crate-private `mime_for_path` over a `phf::Map`, case-insensitive fallback, `application/octet-stream` default; keyed by `safe_relative_components` only |
| `fs/dirbuf.rs` (Plan 316) | platform-neutral `parse_directory_buffer` over the NT `FILE_ID_BOTH_DIR_INFO` wire format; public `parse_directory_buffer`, `DirectoryEntryRecord`, `DirectoryEntryKind`, `DirBufParseError`; the record layout and its byte offsets live here, once |
| `planner.rs` | pure `plan_file_response`, `plan_file_response_with_preconditions`, `plan_file_response_with_preconditions_and_metadata`, conditional/range/ETag/listing helpers; no Hyper types |
| `lib.rs` service | `StaticService::{builder, from_root, root}` + `StaticServiceBuilder::{policy, default_content_type, extra_response_headers, error_policy, listing_limits, build}` (defaults: safe policy, `application/octet-stream`, `Minimal`, 4096 entries / 1 MiB); `impl Service` with `RequestBodyPolicy::Reject` |
| `lib.rs` listing authority (Plan 311) | public `render_directory_listing(entries, max_response_bytes) -> Result<Vec<u8>, ServiceError>` — the single listing renderer (fixed HTML wrapper + one escaped `<li>` link per entry), plus the shared bound constants `DEFAULT_MAX_LISTING_ENTRIES` (4096) and `DEFAULT_MAX_LISTING_RESPONSE_BYTES` (1 MiB) |

## Windows directory enumeration (Plan 316)

The Windows directory listing path reads the NT `FILE_ID_BOTH_DIR_INFO`
record. That record is a **wire format**, not a Windows API binding, so the
parser that owns it is platform-neutral (`fs/dirbuf.rs`) and is exercised on
every platform: unit tests, the fuzz target, and the CI corpus replay.

The layout is written down once — in the parser's module documentation — as
the authoritative NT layout (`winnt.h`, mirrored by `windows-sys` 0.61.2
`Win32::Storage::FileSystem::FILE_ID_BOTH_DIR_INFO`):

| Offset | Size | Field | Type |
|--------|------|-------|------|
| 0 | 4 | `NextEntryOffset` | `u32` |
| 4 | 4 | `FileIndex` | `u32` |
| 8 | 8 | `CreationTime` | `i64` |
| 16 | 8 | `LastAccessTime` | `i64` |
| 24 | 8 | `LastWriteTime` | `i64` |
| 32 | 8 | `ChangeTime` | `i64` |
| 40 | 8 | `EndOfFile` | `i64` |
| 48 | 8 | `AllocationSize` | `i64` |
| 56 | 4 | `FileAttributes` | `u32` |
| 60 | 4 | `FileNameLength` | `u32` |
| 64 | 4 | `EaSize` | `u32` |
| 68 | 1 | `ShortNameLength` | `i8` |
| 70 | 24 | `ShortName` | `[u16; 12]` |
| 96 | 8 | `FileId` | `i64` |
| 104 | 2·N | `FileName` | `[u16; N]` |

The fixed header is **104 bytes** and `FileName` starts at offset 104. The
parser's constants are the single source of truth for those offsets; the
Windows FFI module (`fs/windows.rs`) reads the same record at the same
offsets instead of declaring a second copy of the structure, so the FFI
boundary and the parser cannot drift apart. The parser's entry budget bounds
the number of entries *returned*: a budget of zero yields an empty listing.

The `fuzz/corpus/fuzz_directory_buffer/` seeds are generated from that
layout by `crates/eggserve-static/tests/directory_buffer_corpus.rs`, which
also pins each seed's expected outcome and asserts in routine CI that the
committed corpus is byte-identical to the generator's output.

## Listing authority (Plan 311)

`render_directory_listing` is the only place a directory listing is rendered.
The static service calls it, and every compatibility surface that can serve a
listing (notably the Python bridge) calls the same function with the same
bounds, so the emitted bytes and the denial-of-service ceiling cannot drift
between surfaces.

The renderer refuses (`Err(ServiceError)`) instead of truncating: the
prefix/suffix pre-check and the per-link re-check both enforce
`max_response_bytes`. `DEFAULT_MAX_LISTING_ENTRIES` and
`DEFAULT_MAX_LISTING_RESPONSE_BYTES` are the single source of truth for the
entry and byte ceilings; `eggserve-core::limits` re-exports the entry ceiling
and the Python bridge reads both from this crate.

## Service behavior (owned here)

Absolute-form → 400; non-GET/HEAD → 405 with `Allow: GET, HEAD`;
directory without trailing `/` → 301 preserving query; `index.html` /
`index.htm` lookup; `NotFound` → 404, `Denied` → 403, `IoError` → 500; path
rejections map malformed → 400 else 403; `default_content_type` applies only
when detection yields octet-stream; `extra_response_headers` attach to every
file/directory/error response and never override planned headers; error bodies follow
`ErrorRepresentationPolicy` (`Minimal` fixed text vs `Empty`); every response
passes `normalize_response`. Listing (when enabled) is bounded with escaped
HTML + percent-encoded hrefs.

## Ownership boundaries

- Consumes `ConfinedPath`/`StaticPolicy`, returns handle-carrying
  `BodySource`; duplicates parse-level validation as defense in depth.
- `safe_relative_components` feeds MIME only — never file access.
  Extraction (`from_parts`/`into_parts`/`into_std_file`) ends confinement;
  prefer `into_body`/`into_range_body`.
- Runtime owns framing/admission/streaming (`ResponsePolicy`,
  `max_file_streams` permit, `Date`); server-owned embedding policies
  (Plans 280/282/283) do not move static settings into connection policy.
- Plan 278: `ConfinedPath`/static stays origin-only and rejects
  absolute-form pre-resolution even when the H1 driver opts into
  `OriginOrAbsolute`.

## See also

- [path-confinement](path-confinement.md), [filesystem-confinement](filesystem-confinement.md), [response-planning](response-planning.md), [policy-system](policy-system.md), [primitives-api](primitives-api.md).
- Normative: [docs/secure-root.md](../docs/secure-root.md), [docs/http-response-planning.md](../docs/http-response-planning.md), [docs/http-primitives.md](../docs/http-primitives.md).
