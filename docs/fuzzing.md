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

12 fuzz targets in `fuzz/fuzz_targets/`:

| Target | What it exercises | Key invariants |
|--------|------------------|----------------|
| `request_target` | `ConfinedPath::parse`, `validate_request_target`, `RequestHead` construction | No `..`/`.` components, no NUL, starts with `/`, valid request target, valid method/target/version |
| `percent_decode` | `percent_decode` | No NUL in output, bounded decoded length, valid UTF-8 |
| `path_components` | `split_components`/`validate_components` | No `..`/`.` accepted, no slash/backslash in component, starts with `/` |
| `validate_method` | `Method::new`, `validate_method`/`validate_request_body` | Valid method names, GET/HEAD only for read-only, bodies rejected for read-only methods |
| `range_header` | `evaluate_range_header` | Satisfiable range within file size, start ≤ end, end < file_size |
| `if_none_match` | `evaluate_if_none_match` | Wildcard always matches, matching ETag returns true |
| `platform_component` | `check_component`/`has_windows_drive_prefix`/`is_windows_reserved_name` | Drive prefix requires `X:` pattern, clean components pass |
| `url_parse` | `ParsedUrl::parse` | Scheme is http/https, non-empty host, valid port, path starts with `/`, no fragment |
| `fuzz_header_block` | `HeaderName`, `HeaderValue`, `HeaderBlock` operations | Token-only names, no NUL/CR/LF in values, valid header operations |
| `fuzz_normalize_response` | `StatusCode`, `Response` builder, `normalize_response`, Content-Length reconciliation | Range 100–599, body-forbidden empty, HEAD no body, TE stripped, CL correct |
| `fuzz_request_body` | `RequestBody` state machine | One-shot enforcement, no double-consume |
| `fuzz_directory_buffer` | Directory listing buffer | HTML well-formed, correct link encoding |

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

Corpus filenames must be portable across supported checkout platforms; avoid
Windows device names such as `nul` even when the seed contents are valid.

Seeds are automatically loaded by libFuzzer at startup.

## CI integration

### Normal CI

Property tests run as part of `cargo test` in the standard CI workflow (`.github/workflows/ci.yml`).

### Corpus regression

Corpus regression is part of the standard CI workflow. `cargo test -p eggserve-core --test corpus_replay` replays every committed corpus input through its target logic deterministically, failing on panic or invariant violation.

### Fuzzing workflow

Fuzzing is run manually (not in CI). Each target can be run via `cargo fuzz run <target>` for 60 seconds or longer.

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
6. Verify the new target works with `cargo fuzz run <name>`
