# Invariant Test Matrix

This document enumerates the security and behavioral invariants enforced by eggserve, organized by category. Each invariant maps to specific test coverage in the Rust and Python test suites.

## Request target invariants

| Invariant | Test coverage |
|-----------|---------------|
| Only origin-form request targets accepted | `integration.rs` — request target validation tests |
| Absolute-form rejected | `integration.rs` — `request_target_rejects_absolute_form` |
| Authority-form rejected | `integration.rs` — `request_target_rejects_authority_form` |
| Asterisk-form rejected | `integration.rs` — `request_target_rejects_asterisk_form` |
| Malformed percent encoding rejected | `integration.rs` — `request_target_rejects_malformed_percent` |
| Percent-encoded traversal rejected | `integration.rs` — `request_target_rejects_encoded_traversal` |
| NUL rejected | `integration.rs` — `request_target_rejects_nul` |
| Backslash rejected by default | `integration.rs` — `request_target_rejects_backslash` |
| Windows drive prefixes rejected | `integration.rs` — `request_target_rejects_windows_drive` |
| Windows ADS syntax rejected | `integration.rs` — `request_target_rejects_ads_syntax` |
| Windows reserved names rejected | `integration.rs` — `request_target_rejects_reserved_name` |
| ConfinedPath preserves the policy used during parsing | `path/mod.rs` — `path_policy_returns_parsed_policy` |

## Policy invariants

| Invariant | Test coverage |
|-----------|---------------|
| Dotfiles denied by default | `primitives/mod.rs` — `static_policy_default_denies_all`; `test_primitives.py` — `TestStaticPolicy.test_defaults` |
| Dotfiles allowed only through explicit policy | `secure_root.rs` — `resolve_dotfile_allowed_when_policy_permits`; `test_primitives.py` — `TestSecureRoot.test_dotfile_allowed` |
| Directory listing disabled by default | `primitives/mod.rs` — `static_policy_default_denies_all` |
| Directory listing enabled only through explicit policy | `integration.rs` — directory listing tests |
| Symlinks denied by default | `primitives/mod.rs` — `static_policy_default_denies_all` |
| Follow-symlinks mode explicit and documented weaker | `secure_root.rs` — `resolve_symlink_allowed_when_follow_enabled` |

## Filesystem invariants

| Invariant | Test coverage |
|-----------|---------------|
| Normal file resolves | `secure_root.rs` — `resolve_normal_file`; `test_primitives.py` — `TestSecureRoot.test_resolve_file` |
| Normal directory resolves | `secure_root.rs` — `resolve_normal_directory`; `test_primitives.py` — `TestSecureRoot.test_resolve_directory` |
| Missing path is not found | `secure_root.rs` — `resolve_missing_path`; `test_primitives.py` — `TestSecureRoot.test_resolve_not_found` |
| Final symlink denied under safe defaults on Unix | `secure_root.rs` — `resolve_symlink_denied_under_defaults` (cfg(unix)) |
| Intermediate symlink denied under safe defaults on Unix | `secure_root.rs` — `resolve_intermediate_symlink_denied` (cfg(unix)) |
| Symlink swap test / equivalent no-follow kernel behavior test remains present on Unix | `integration.rs` — symlink swap / TOCTOU tests (cfg(unix)) |
| Follow-symlinks internal target allowed if policy permits | `secure_root.rs` — `resolve_symlink_allowed_when_follow_enabled` (cfg(unix)) |
| Follow-symlinks outside-root target denied | `secure_root.rs` — `resolve_outside_root_symlink_denied_when_follow_enabled` (cfg(unix)) |
| Directory listings hide dotfiles and symlinks under safe defaults | `secure_root.rs` — `directory_list_hides_dotfiles_under_defaults`, `directory_listing_hides_symlinks_under_defaults` (cfg(unix)); `test_primitives.py` — `TestSecureRoot.test_list_hides_dotfiles` |
| File serving path does not reopen by absolute path under Unix safe defaults | `secure_root.rs` — tests use `into_std_file()` / `into_parts()` confirming handle-based access; `integration.rs` — response tests confirm file handle origin |
| ConfinedPath preserves the policy used during parsing | `path/mod.rs` — `path_policy_returns_parsed_policy` |

## HTTP validation invariants

| Invariant | Test coverage |
|-----------|---------------|
| Only GET/HEAD accepted for static serving | `integration.rs` — method validation tests |
| Other methods map to 405-equivalent result | `integration.rs` — method not allowed tests |
| Positive Content-Length rejected under zero-body policy | `integration.rs` — body validation tests |
| Invalid Content-Length rejected | `integration.rs` — body validation tests |
| Transfer-Encoding rejected for GET/HEAD | `integration.rs` — body validation tests |
| Conflicting Content-Length and Transfer-Encoding rejected | `integration.rs` — body validation tests |

## Response planning invariants

| Invariant | Test coverage |
|-----------|---------------|
| GET file plan includes status, content length, content type, validators, and nosniff | `planner.rs` — unit tests; `integration.rs` — response header tests |
| HEAD file plan has matching headers and empty body | `integration.rs` — HEAD parity tests |
| Matching ETag conditional returns 304 | `planner.rs` — conditional request tests; `integration.rs` — 304 tests |
| Matching Last-Modified conditional returns 304 when appropriate | `planner.rs` — conditional request tests |
| Satisfiable range returns 206 with correct content range | `planner.rs` — range request tests; `integration.rs` — 206 tests |
| Unsatisfiable range returns 416 with correct content range | `planner.rs` — range request tests; `integration.rs` — 416 tests |
| Directory listing HTML escapes visible names | `response.rs` — directory listing HTML tests |
| Directory listing hrefs percent-encode path segments | `response.rs` — directory listing HTML tests |
| Directory listing response includes CSP and referrer policy | `integration.rs` — directory listing header tests |
| Range 206 includes content-type, accept-ranges, etag, last-modified | `planner.rs` — `plan_file_response_range_206` |
| Range 416 includes content-length: 0, accept-ranges, content-range | `planner.rs` — `plan_file_response_range_416` |

## Body-source invariants

| Invariant | Test coverage |
|-----------|---------------|
| Full file body source streams exact bytes | `primitives/body.rs` — `file_full_body_source` |
| Range body source streams exact bytes | `primitives/body.rs` — `file_range_body_source`, `file_range_body_source_middle` |
| Empty body source produces zero bytes | `primitives/body.rs` — `empty_body_source` |
| Bytes body source produces exact bytes | `primitives/body.rs` — `bytes_body_source` |
| Range bounds are checked | `primitives/body.rs` — `read_range_inverted_returns_empty`; `secure_root.rs` — `into_body_range_invalid` |
| Consuming conversion prevents double-use | `secure_root.rs` — `into_body_consumes_file` |
| No path reopening in safe-default static path | Structural invariant: `into_body()` and `into_range_body()` consume the file handle directly from `ResolvedFile`, never reconstruct a path |
| Denied resources cannot produce body sources | `test_primitives.py` — denied resources return `Denied`, not `File` |
| Python body source exposes correct kind | `test_primitives.py` — `test_body_source_repr` |
| Python body source read returns expected bytes | `test_primitives.py` — `test_body_for_plan_read_range` |

## Streaming buffer invariants (Plan 088)

| Invariant | Test coverage |
|-----------|---------------|
| Each chunk allocates a fresh bounded buffer (8 KiB default) | `response.rs` — `DEFAULT_CHUNK_SIZE` constant |
| Chunk buffer never exceeds remaining range bytes | `streaming_buffer_qualification` — exact range boundary tests |
| No stale cross-request data exposure | `streaming_buffer_qualification` — `buffer_isolation` |
| Client disconnect releases stream permit | `streaming_buffer_qualification` — `client_disconnect_releases_stream_permits` |
| Forced shutdown releases all stream permits | `streaming_buffer_qualification` — `forced_shutdown_releases_stream_permits` |
| Concurrent stream exhaustion returns 503 | `streaming_buffer_qualification` — `concurrent_stream_exhaustion_returns_503` |
| Range request releases permit after body consumption | `streaming_buffer_qualification` — `range_request_releases_permits_after_stream` |
| HEAD does not acquire stream permits | `streaming_buffer_qualification` — `head_request_does_not_acquire_stream_permits` |
| normalize_metadata uses in-place retain (no clone) | `canonical.rs` — `strip_hop_by_hop`, `remove_header` via `retain` |
| Hop-by-hop stripping preserves duplicate non-hop-by-hop headers | `canonical.rs` — `normalize_metadata_preserves_duplicate_non_hop_by_hop_headers` |

## Python binding invariants

| Invariant | Test coverage |
|-----------|---------------|
| Python behavior mirrors Rust behavior for all above categories where platform permits | `test_primitives.py` — full primitive test suite; `test_server.py` — subprocess API tests |
| Python cannot directly construct `ResolvedFile` or `ResolvedDirectory` from arbitrary paths | `test_primitives.py` — access tests confirm only `resource.file` / `resource.directory` paths |
| Python exceptions expose stable machine-readable codes | `test_primitives.py` — exception hierarchy tests |
| Python response plans expose plain status/header/body-plan values, not Hyper internals | `test_primitives.py` — `TestResponsePlan` tests confirming `ResponsePlan` namedtuple fields |
| PathPolicy from RequestTarget.parse() survives resolve() | `test_primitives.py` — `test_request_target_dotfile_resolves_with_matching_policy` |
| PathPolicy does not override StaticPolicy serving decisions | `test_primitives.py` — `test_request_target_path_policy_does_not_override_static_policy` |
| resolve_path(path_policy=...) is honored | `test_primitives.py` — `test_resolve_path_with_path_policy` |
| resolve_path explicit path_policy does not bypass StaticPolicy | `test_primitives.py` — `test_resolve_path_explicit_path_policy_does_not_bypass_static_policy` |
| ResolvedDirectory.list() preserves allow_dotfiles policy | `test_primitives.py` — `test_directory_list_preserves_dotfile_policy` |
| ResolvedDirectory.resolve_child() preserves allow_dotfiles policy | `test_primitives.py` — `test_directory_resolve_child_with_dotfile_policy` |

## HTTP primitive invariants

| Invariant | Test coverage |
|-----------|---------------|
| GET/HEAD method validation returns ReadOnlyMethod | `primitives/http.rs` — method validation tests |
| Unsupported methods rejected with MethodNotAllowed | `primitives/http.rs` — method rejection tests |
| Origin-form request target accepted | `primitives/http.rs` — request target tests |
| Empty/absolute/asterisk/whitespace targets rejected | `primitives/http.rs` — request target rejection tests |
| Zero Content-Length allowed under zero-body policy | `primitives/http.rs` — body validation tests |
| Positive Content-Length rejected under zero-body policy | `primitives/http.rs` — body validation tests |
| Malformed Content-Length rejected | `primitives/http.rs` — body validation tests |
| Non-empty Transfer-Encoding rejected | `primitives/http.rs` — body validation tests |
| Conflicting Content-Length and Transfer-Encoding rejected | `primitives/http.rs` — body validation tests |
| Configurable max_body_bytes enforced | `primitives/http.rs` — body validation with max_body_bytes |
| ETag weak comparison matches strong | `primitives/planner.rs` — ETag comparison tests |
| If-None-Match wildcard matches any ETag | `primitives/planner.rs` — wildcard test |
| If-None-Match list matching | `primitives/planner.rs` — list matching tests |
| If-Modified-Since future date triggers 304 | `primitives/planner.rs` — IMS future test |
| If-Modified-Since past date triggers 200 | `primitives/planner.rs` — IMS past test |
| Malformed If-Modified-Since ignored | `primitives/planner.rs` — malformed IMS test |
| Range bytes=0-0 returns first byte | `primitives/planner.rs` — single byte range test |
| Range bytes=0- returns to EOF | `primitives/planner.rs` — open-ended range test |
| Range bytes=-N returns last N bytes | `primitives/planner.rs` — suffix range test |
| Range suffix exceeding file returns whole file | `primitives/planner.rs` — suffix exceeds file test |
| Range start beyond EOF returns 416 | `primitives/planner.rs` — start beyond EOF test |
| Range start > end returns 416 | `primitives/planner.rs` — inverted range test |
| Multiple ranges fall through to 200 | `primitives/planner.rs` — multiple ranges test |
| Unsupported range unit falls through to 200 | `primitives/planner.rs` — unsupported unit test |
| Zero-length file range returns 416 | `primitives/planner.rs` — zero-length file test |
| If-Range matching ETag serves 206 | `primitives/planner.rs` — If-Range matching test |
| If-Range non-matching ETag serves 200 | `primitives/planner.rs` — If-Range mismatch test |
| If-Range matching date serves 206 | `primitives/planner.rs` — If-Range date match test |
| If-Range stale date serves 200 | `primitives/planner.rs` — If-Range stale date test |
| HEAD with range returns headers but no body | `primitives/planner.rs` — HEAD range test |
| HEAD parity: same status, headers, empty body | `primitives/planner.rs` — HEAD parity tests; `integration.rs` — HEAD parity tests |
| Live TCP: GET file returns 200 with body | `http_primitives_integration.rs` — live GET test |
| Live TCP: HEAD file returns 200 no body | `http_primitives_integration.rs` — live HEAD test |
| Live TCP: missing file returns 404 | `http_primitives_integration.rs` — live 404 test |
| Live TCP: dotfile returns 403 | `http_primitives_integration.rs` — live dotfile test |
| Live TCP: POST returns 405 with Allow header | `http_primitives_integration.rs` — live 405 test |
| Live TCP: malformed percent returns 400 | `http_primitives_integration.rs` — live 400 test |
| Live TCP: traversal returns 403 | `http_primitives_integration.rs` — live traversal test |
| Live TCP: positive Content-Length returns 413 | `http_primitives_integration.rs` — live 413 test |
| Live TCP: invalid Content-Length returns 400 | `http_primitives_integration.rs` — live invalid CL test |
| Live TCP: Range returns 206 with correct body | `http_primitives_integration.rs` — live range test |
| Live TCP: unsatisfiable range returns 416 | `http_primitives_integration.rs` — live 416 test |
| Live TCP: conditional ETag returns 304 | `http_primitives_integration.rs` — live 304 test |
| Live TCP: HEAD range returns 206 no body | `http_primitives_integration.rs` — live HEAD range test |
