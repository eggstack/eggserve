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
- Modules (`src/`): `path/` (public), `fs/` + `secure_root.rs` + `mime.rs`
  + `planner.rs` (crate-private, re-exported at root per `lib.rs`).

## Module inventory

| Area | Contents |
|------|----------|
| `path/` (`mod`/`components`/`decode`/`platform`/`policy`/`rejected`) | `ConfinedPath::{parse, from_path_component, as_str, components, path_policy}`, `PathPolicy{dotfiles, reject_backslash}`, parse-level `DotfilePolicy::{Denied, Allow}`, 17-variant `PathRejection` |
| `secure_root.rs` + `fs/{mod,unix,windows}` | `SecureRoot::{new, policy, root_path, resolve, resolve_uri}`, `resolve_and_plan`, `ResolvedResource::{File, Directory, NotFound, Denied, IoError}`, `ResourceDeniedReason`, `ResolvedFile::{len, modified, metadata, content_type, plan_response, into_body, into_range_body}` (+ gated `into_std_file`/`into_parts`/`from_parts`), `ResolvedDirectory::{components, list, resolve_child}`; internal `PinnedRoot`/`RootGuard`; Unix `statat` + `openat(O_NOFOLLOW)`, Windows handle-relative opens |
| `mime.rs` | crate-private `mime_for_path` over a `phf::Map`, case-insensitive fallback, `application/octet-stream` default; keyed by `safe_relative_components` only |
| `planner.rs` | pure `plan_file_response`, `plan_file_response_with_preconditions`, `plan_file_response_with_preconditions_and_metadata`, conditional/range/ETag/listing helpers; no Hyper types |
| `lib.rs` service | `StaticService::{builder, from_root, root}` + `StaticServiceBuilder::{policy, default_content_type, extra_response_headers, error_policy, listing_limits, build}` (defaults: safe policy, `application/octet-stream`, `Minimal`, 4096 entries / 1 MiB); `impl Service` with `RequestBodyPolicy::Reject` |

## Service behavior (owned here)

Absolute-form → 400; non-GET/HEAD → 405 with `Allow: GET, HEAD`;
directory without trailing `/` → 301 preserving query; `index.html` /
`index.htm` lookup; `NotFound` → 404, `Denied` → 403, `IoError` → 404; path
rejections map malformed → 400 else 403; `default_content_type` applies only
when detection yields octet-stream; `extra_response_headers` attach to final
200 only and never override planned headers; error bodies follow
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
