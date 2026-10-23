# Fuzz Targets

Fuzz targets for eggserve's path confinement, request parsing, response planning, and HTTP field validation.

## Targets

| Target | What it covers |
|--------|---------------|
| `request_target` | HTTP origin-form parsing, path confinement, request target validation, request head construction |
| `percent_decode` | Single-pass percent decoding, malformed encodings, invalid UTF-8 |
| `path_components` | Path normalization, component validation, encoded dot-components |
| `validate_method` | Method construction, validation, and body rejection for read-only methods |
| `range_header` | Range request parsing: suffix, open-ended, start-end, clamping, zero-size files |
| `if_none_match` | ETag matching: weak/strong comparison, wildcard, comma-separated lists |
| `platform_component` | Windows reserved names, drive prefixes, alternate data streams |
| `fuzz_header_block` | HeaderName, HeaderValue, and HeaderBlock operations |
| `fuzz_normalize_response` | StatusCode validation, response building, response normalization, Content-Length reconciliation |
| `fuzz_request_body` | RequestBody state machine, one-shot enforcement |
| `fuzz_directory_buffer` | NT `FILE_ID_BOTH_DIR_INFO` directory-buffer parsing (runs on every platform — the record layout is a wire format, not a Windows API binding) |

## Running

Requires `cargo-fuzz` (install with `cargo install cargo-fuzz`):

```sh
cargo fuzz run request_target
cargo fuzz run percent_decode
cargo fuzz run path_components
cargo fuzz run validate_method
cargo fuzz run range_header
cargo fuzz run if_none_match
cargo fuzz run platform_component
cargo fuzz run fuzz_header_block
cargo fuzz run fuzz_normalize_response
cargo fuzz run fuzz_request_body
cargo fuzz run fuzz_directory_buffer
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
- Range parser: satisfiable range within file size, start <= end, end < file_size
- ETag matching: wildcard always matches, matching ETag returns true
- Platform checks: drive prefix requires `X:` pattern, clean components pass
- Header validation: no NUL/CR/LF in values, empty names rejected
- Response normalization: body-forbidden statuses empty, TE stripped, CL correct
- Directory buffer: entry budget bounds the listing, a zero budget is an empty listing, dotfile flag
  matches the parsed name, `.`/`..` never leak into a listing, the parse is deterministic, and a rejected
  buffer stays rejected under a wider entry budget

Every assertion must be able to fail: no variant match over every enum variant, and no
`!x.is_empty() || x.is_empty()`.

## Seed corpora

Each target has a `corpus/<target>/` directory with hand-crafted seeds covering:
- Normal valid inputs
- Edge cases (empty, max-length, boundary values)
- Malformed inputs (truncated, special chars, traversal attempts)
- Regression inputs from existing test suites

The `fuzz_directory_buffer` corpus is generated rather than hand-written: it is built from the
authoritative NT `FILE_ID_BOTH_DIR_INFO` layout by
`crates/eggserve-static/tests/directory_buffer_corpus.rs`, which records the generator, pins each
seed's expected outcome, and asserts in routine CI that the committed corpus is byte-identical to
the generator's output. Regenerate it with:

```sh
cargo test -p eggserve-static --test directory_buffer_corpus -- --ignored
```

The seeds cover the happy path (a valid single entry, a multi-entry buffer, a directory entry, a
reparse-point entry, a dotfile entry) and every rejection (a truncated header, an odd file-name
length, an over-long file name, an offset overflow, an offset underflow, invalid UTF-16), plus an
empty buffer, the `.`/`..` pseudo-entries and a pseudo-random blob.

## Property tests

Deterministic property tests using proptest run in normal CI via `cargo test`. See [docs/fuzzing.md](../docs/fuzzing.md) for the full inventory.

## CI integration

- **Normal CI**: Property tests run as part of `cargo test`. Corpus regression tests via `cargo test -p eggserve-core --test corpus_replay` replay every committed seed.
- **Fuzz build gate**: CI compiles this crate with `cd fuzz && cargo check --bins`, so a target that
  no longer builds against the current API fails CI instead of rotting. The crate is its own
  workspace and has its own lockfile; it never touches the repository-root `Cargo.lock`.
- **Manual fuzzing**: `cargo fuzz run <target>` for extended fuzzing sessions
