# Binary Size Tracking

Active document tracking artifact sizes across profiles. Updated by Plan 105.

## Environment

- Target: `x86_64-unknown-linux-gnu`
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Commit: `d5f873a1e66191b97692711e946cf4a687edb5b4`

## Baseline (before Plan 105)

| Artifact | Profile | Stripped | Size (bytes) |
|----------|---------|----------|-------------|
| Default CLI | release | no | 2,219,904 |
| TLS CLI | release | no | 3,324,096 |

## Final (after Plan 105)

| Artifact | Profile | Stripped | Size (bytes) | Change |
|----------|---------|----------|-------------|--------|
| Default CLI | release | no | 2,052,240 | -7.6% |
| Default CLI | dist (opt-level=z) | yes | 885,760 | -60.1% |
| TLS CLI | release | no | 3,155,944 | -5.1% |
| TLS CLI | dist (opt-level=z) | yes | 1,246,184 | -62.5% |

## Changes applied

| Change | Acceptance | Notes |
|--------|-----------|-------|
| `profile.dist` (opt-level=z, fat LTO, codegen-units=1, strip=symbols) | accepted | >50% reduction on dist builds |
| Current-thread CLI runtime (`Builder::new_current_thread()`) | accepted | 5-8% reduction, no behavioral regression |
| Tokio feature narrowing (remove `signal`, gate `rt-multi-thread` behind `client`) | accepted | Dependency hygiene, no byte savings measured in isolation |
| PHF MIME map retained | accepted | Simple, correct, no build machinery |
| Error strings retained | accepted | Auditability > micro-optimization |

## Rejected experiments

None. All experiments were accepted.

## Reproduction commands

```sh
# Baseline
cargo clean && cargo build --release --locked -p eggserve-bin
stat --printf='%s\n' target/release/eggserve

# Dist build
cargo build --profile dist --locked -p eggserve-bin
stat --printf='%s\n' target/dist/eggserve

# TLS dist
cargo build --profile dist --locked -p eggserve-bin --features tls
stat --printf='%s\n' target/dist/eggserve

# Feature graph check
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-core --no-default-features
```
