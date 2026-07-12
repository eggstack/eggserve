# Fuzzing and Property Testing

eggserve uses two layers of automated testing beyond hand-written unit tests:

1. **Property tests** (proptest) — run in normal CI via `cargo test`, bounded inputs, deterministic
2. **Fuzz targets** (libFuzzer) — scheduled/manual CI, unbounded exploration, crash artifact upload

## Property tests

Located in `#[cfg(test)]` modules across `crates/eggserve-core/src/`:

| Module | Property tested |
|--------|----------------|
| `path/decode.rs` | No NUL in output, bounded length, valid UTF-8, no panic on arbitrary input |
| `path/platform.rs` | `check_component`/`is_windows_reserved_name`/`has_windows_drive_prefix` never panic; case-insensitivity; drive-prefix structure; clean components pass |
| `primitives/planner.rs` | Range within file size, ETag format, HEAD no body, 304 empty body, weak/strong ETag equivalence, wildcard always matches, no panic on arbitrary range/ETag strings |
| `primitives/client/url.rs` | Successful parse invariants (scheme, host, path, no fragment); rejected URLs never panic; display roundtrip; is_https consistency; no panic on arbitrary input |
| `primitives/client/request.rs` | `validate_header` never panic; valid names accepted; empty name rejected; NUL/CR/LF in value rejected; `is_token_byte` correctness |
| `response.rs` | `html_escape` no raw angle brackets, no panic; `percent_encode_path_segment` no raw `?`/`#`, no panic |

Run with:
```sh
cargo test -p eggserve-core
```

## Fuzz targets

Nine fuzz targets in `fuzz/fuzz_targets/`:

| Target | What it exercises | Key invariants |
|--------|------------------|----------------|
| `request_target` | `ConfinedPath::parse` origin-form parsing | No `..`/`.` components, no NUL, starts with `/` |
| `percent_decode` | `percent_decode` | No NUL in output, bounded decoded length, valid UTF-8 |
| `path_components` | `split_components`/`validate_components` | No `..`/`.` accepted, no slash/backslash in component, starts with `/` |
| `url_parse` | `ParsedUrl::parse` | Scheme is http/https, non-empty host, valid port, path starts with `/`, no fragment |
| `range_header` | `evaluate_range_header` | Satisfiable range within file size, start ≤ end, end < file_size |
| `if_none_match` | `evaluate_if_none_match` | Wildcard always matches, matching ETag returns true |
| `platform_component` | `check_component`/`has_windows_drive_prefix`/`is_windows_reserved_name` | Drive prefix requires `X:` pattern, clean components pass |
| `validate_request_target` | `validate_request_target` | Starts with `/`, no whitespace |
| `validate_method` | `validate_method`/`validate_request_body` | GET/HEAD only, bodies rejected for read-only methods |

Run a single target:
```sh
cd fuzz
cargo fuzz run url_parse          # default 60s
cargo fuzz run range_header -- -max_total_time=300  # 5 minutes
```

## Seed corpora

`fuzz/corpus/<target>/` contains hand-crafted seeds for each target. Seeds cover:
- Normal valid inputs
- Edge cases (empty, max-length, boundary values)
- Malformed inputs (truncated, special chars, traversal attempts)
- Regression inputs from existing test suites

Seeds are automatically loaded by libFuzzer at startup.

## CI integration

### Normal CI

Property tests run as part of `cargo test` in the standard CI workflow (`.github/workflows/ci.yml`).

### Scheduled fuzz runs

`.github/workflows/fuzz.yml` runs weekly (Monday 3:00 UTC) or on manual dispatch:
- Each target runs for 60 seconds (configurable via workflow dispatch)
- Crash artifacts are uploaded and retained for 30 days
- Manual dispatch can target a specific target and duration

## Failure handling

When a fuzz target finds a crash:

1. **Minimize**: `cargo fuzz merge <target>` to reduce the input
2. **Reproduce**: Add the minimal input to `fuzz/corpus/<target>/` as a regression seed
3. **Classify**: Determine if the failure is a security issue (path escape, OOB, panic) or a correctness issue
4. **Fix**: Patch the root cause in the affected module
5. **Verify**: Re-run the fuzz target to confirm the fix; the corpus seed prevents regression

## Adding a new fuzz target

1. Create `fuzz/fuzz_targets/<name>.rs` with a `fuzz_target!(|data: &[u8]| { ... })` entry point
2. Parse the fuzz input (typically `std::str::from_utf8` or manual splitting)
3. Call the target function and assert invariants on the output
4. Add a `[[bin]]` section to `fuzz/Cargo.toml`
5. Create `fuzz/corpus/<name>/` with seed files
6. Add the target to `.github/workflows/fuzz.yml` matrix
