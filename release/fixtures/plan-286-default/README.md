# Plan 286 default registry consumer

This standalone consumer pins only published crates.io artifacts. It verifies
EggServe-owned policy/admission and parser ceilings remain the defaults, then
runs the direct H1 tunnel upgrade, CONNECT, read-ahead, bounds, admission, and
shutdown suite.

```sh
cargo test --locked --manifest-path release/fixtures/plan-286-default/Cargo.toml
cargo tree --locked --manifest-path release/fixtures/plan-286-default/Cargo.toml -e no-dev
```
