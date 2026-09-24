# Plan 286 advanced registry consumer

This standalone consumer pins published crates.io artifacts and proves the
caller-owned TLS/H1 seam, explicit deadline/semantic-limit and admission
ownership, rejection presentation, absolute-form dispatch, body/trailer flow,
tunnel behavior, and shutdown. It contains no path/git/patch dependency.

```sh
cargo test --locked --manifest-path release/fixtures/plan-286-advanced/Cargo.toml
cargo tree --locked --manifest-path release/fixtures/plan-286-advanced/Cargo.toml -e no-dev
```
