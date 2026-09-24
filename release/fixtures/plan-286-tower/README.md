# Plan 286 Tower registry consumer

The feature-gated suites prove Axum/Tower composition, request/response
streaming and trailers, duplicate headers, lifecycle cancellation, and
compatibility adapter behavior using only published crates.io packages.

```sh
cargo test --locked --manifest-path release/fixtures/plan-286-tower/Cargo.toml --features tower
cargo tree --locked --manifest-path release/fixtures/plan-286-tower/Cargo.toml --features tower -e no-dev
```
