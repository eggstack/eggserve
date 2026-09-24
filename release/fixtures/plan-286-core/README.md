# Plan 286 core compatibility consumer

This standalone crates.io-only consumer proves established `eggserve_core`
module paths still compose with the direct primitive, server, and static crate
facades. The default static service test confirms dotfiles remain denied; the
optional Tower feature test exercises the forwarded adapter against loopback H1.

```sh
cargo test --locked --manifest-path release/fixtures/plan-286-core/Cargo.toml
cargo test --locked --manifest-path release/fixtures/plan-286-core/Cargo.toml --features tower
cargo tree --locked --manifest-path release/fixtures/plan-286-core/Cargo.toml -e no-dev
```
