# Fuzz Targets

Initial fuzz targets for eggserve's path confinement module.

## Running

```sh
cargo fuzz run request_target
cargo fuzz run percent_decode
cargo fuzz run path_components
```

## Assertions

- Parser never panics on arbitrary input
- Accepted paths contain no parent/current components
- Accepted paths contain no NUL bytes
- Percent decoder never double-decodes
