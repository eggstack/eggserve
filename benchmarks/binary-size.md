# Binary Size Tracking

This is the Plan 108 corrective-pass measurement snapshot. Sizes are recorded
separately for the unstripped `release` profile and the stripped `dist`
distribution profile; those profiles must not be compared as if the difference
were solely code-size change.

## Environment

- Target: `x86_64-unknown-linux-gnu`
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Candidate SHA: `0379a3d` (`fix: close static metadata and runtime ownership gaps`)
- Maturin: 1.14.1

## CLI artifacts

| Artifact | Profile | Stripped | Size (bytes) |
|----------|---------|----------|-------------:|
| Default CLI | release | no | 1,964,616 |
| Default CLI | dist | yes | 857,400 |
| TLS CLI | release | no | 3,077,856 |
| TLS CLI | dist | yes | 1,218,464 |

The `dist` profile is approximately 56.6% smaller than this snapshot's
unstripped default release artifact and 60.3% smaller for TLS. The comparison
is profile-aware: `dist` uses size optimization, single-unit LTO, one codegen
unit, and symbol stripping.

## Python wheel artifacts

Measured from the CPython 3.14 Linux wheel built by
`scripts/test-python-wheel.sh`:

| Artifact | Measurement | Size (bytes) |
|----------|-------------|-------------:|
| Bundled CLI | on-disk `dist` binary | 1,218,464 |
| Native extension | uncompressed wheel member | 2,256,096 |
| Wheel | `.whl` file on disk | 1,747,905 |

The wheel also contains Python sources, metadata, and the bundled CLI. The
native-extension value is uncompressed while the wheel value is compressed;
they are intentionally reported as different measurements.

## Current-thread runtime evidence

The standalone CLI uses Tokio's current-thread runtime. A bounded local smoke
measurement on this candidate served an exact 1 KiB file with 16 client workers
and 1,000 fresh HTTP/1.1 requests, repeated three times:

| Sample | Elapsed (s) | Requests/s |
|--------|------------:|-----------:|
| 1 | 0.4987 | 2,005.3 |
| 2 | 0.5501 | 1,818.0 |
| 3 | 0.4924 | 2,030.8 |

This is a suitability smoke measurement, not a release gate or a cross-machine
benchmark. The standalone CLI has no multi-thread runtime build variant; the
embedded runtime's current-thread and multi-thread lifecycle coverage passes in
`lifecycle_integration.rs`. Large-file and range correctness are covered by
the streaming and wire suites rather than treated as a permanent performance
gate.

## Reproduction commands

```sh
cargo build --release --locked -p eggserve-bin
stat --printf='%s\n' target/release/eggserve

cargo build --profile dist --locked -p eggserve-bin
stat --printf='%s\n' target/dist/eggserve

cargo build --release --locked -p eggserve-bin --features tls
stat --printf='%s\n' target/release/eggserve

cargo build --profile dist --locked -p eggserve-bin --features tls
stat --printf='%s\n' target/dist/eggserve

PYTHON=python3.14 bash scripts/test-python-wheel.sh
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-core --no-default-features
```

The wheel script uses a controlled temporary fixture for its bundled CLI
smoke, so release verification does not depend on a repository directory
listing.
