# Plan 276 direct Axum consumer

This standalone fixture is outside the Cargo workspace and depends on the direct
`eggserve-server` package only. It exercises a pre-bound H1 listener, TowerToEggserve
with `RequestBodyPolicy`, incremental request/response streaming, duplicate headers,
middleware, disconnect cancellation, and typed control/completion shutdown.

Run from this directory with `cargo test --locked`. When validating a release
candidate, replace the local path dependency with its exact staged or registry
version and retain the same tests.
