# Fuzz Targets

Fuzz targets for eggserve's path confinement and request parsing modules.

## Targets

| Target | What it covers |
|--------|---------------|
| `request_target` | HTTP origin-form parsing, query stripping, component validation |
| `percent_decode` | Single-pass percent decoding, malformed encodings, invalid UTF-8 |
| `path_components` | Path normalization, component validation, encoded dot-components |

## Running

Requires `cargo-fuzz` (install with `cargo install cargo-fuzz`):

```sh
cargo fuzz run request_target
cargo fuzz run percent_decode
cargo fuzz run path_components
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

## Coverage notes

These targets cover the path confinement and request parsing layers only. Filesystem traversal (symlink resolution, canonical-root verification) is not fuzzed because it requires a deterministic fixture model. Body metadata parsing (`Content-Length`/`Transfer-Encoding` validation) is covered by unit tests in `service.rs` but does not have a dedicated fuzz target.
