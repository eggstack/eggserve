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
