# Fuzz Targets

Fuzz targets for eggserve's path confinement, request parsing, URL parsing, response planning, and client primitives.

## Targets

| Target | What it covers |
|--------|---------------|
| `request_target` | HTTP origin-form parsing, query stripping, component validation |
| `percent_decode` | Single-pass percent decoding, malformed encodings, invalid UTF-8 |
| `path_components` | Path normalization, component validation, encoded dot-components |
| `url_parse` | Hand-rolled URL parser: scheme, host, port, path, authority, fragment stripping |
| `range_header` | Range request parsing: suffix, open-ended, start-end, clamping, zero-size files |
| `if_none_match` | ETag matching: weak/strong comparison, wildcard, comma-separated lists |
| `platform_component` | Windows reserved names, drive prefixes, alternate data streams |
| `validate_request_target` | Request target validation: starts-with-/no-whitespace |
| `validate_method` | Method validation and request body rejection for read-only methods |
| `fuzz_directory_buffer` | Windows directory buffer parser: FILE_ID_BOTH_DIR_INFO parsing, offset validation, UTF-16 decoding (Windows-only) |

## Running

Requires `cargo-fuzz` (install with `cargo install cargo-fuzz`):

```sh
cargo fuzz run request_target
cargo fuzz run percent_decode
cargo fuzz run path_components
cargo fuzz run url_parse
cargo fuzz run range_header
cargo fuzz run if_none_match
cargo fuzz run platform_component
cargo fuzz run validate_request_target
cargo fuzz run validate_method
cargo fuzz run fuzz_directory_buffer  # Windows-only
```

To run with a time limit:

```sh
cargo fuzz run request_target -- -max_total_time=60
```

## Assertions

- Parser never panics on arbitrary input
- Accepted paths contain no parent/current components (`..`, `.`)
- Accepted paths contain no NUL bytes
- Percent decoder never double-decodes
- No path component contains decoded NUL bytes
- URL parser: scheme is http/https, host non-empty, path starts with `/`, no fragments
- Range parser: satisfiable range within file size, start <= end, end < file_size
- ETag matching: wildcard always matches, matching ETag returns true
- Platform checks: drive prefix requires `X:` pattern, clean components pass
- Header validation: no NUL/CR/LF in values, empty names rejected

## Seed corpora

Each target has a `corpus/<target>/` directory with hand-crafted seeds covering:
- Normal valid inputs
- Edge cases (empty, max-length, boundary values)
- Malformed inputs (truncated, special chars, traversal attempts)
- Regression inputs from existing test suites

## Property tests

Deterministic property tests using proptest run in normal CI via `cargo test`. See [docs/fuzzing.md](../docs/fuzzing.md) for the full inventory.

## CI integration

- **Normal CI**: Property tests run as part of `cargo test`. Corpus regression tests in `.github/workflows/fuzz-replay.yml` replay every committed seed on every PR and push to main.
- **Scheduled fuzz**: `.github/workflows/fuzz.yml` runs weekly (Monday 3:00 UTC) or on manual dispatch, 60s per target, crash artifacts uploaded
