# Binary Size Tracking

This is the Plan 107 corrective-pass measurement snapshot. Sizes are recorded
separately for the unstripped `release` profile and the stripped `dist`
distribution profile; those profiles must not be compared as if the difference
were solely code-size change.

## Environment

- Target: `x86_64-unknown-linux-gnu`
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Candidate SHA: record from `git rev-parse HEAD` after the implementation commit

## CLI artifacts

| Artifact | Profile | Stripped | Size (bytes) |
|----------|---------|----------|-------------:|
| Default CLI | release | no | 1,972,360 |
| Default CLI | dist | yes | 857,536 |
| TLS CLI | release | no | 3,072,440 |
| TLS CLI | dist | yes | 1,218,568 |

The `dist` profile is approximately 56.6% smaller than this snapshot's
unstripped default release artifact and 60.3% smaller for TLS. The comparison
is profile-aware: `dist` uses size optimization, single-unit LTO, one codegen
unit, and symbol stripping.

## Python wheel artifacts

Measured from the CPython 3.14 Linux wheel built by
`scripts/test-python-wheel.sh`:

| Artifact | Measurement | Size (bytes) |
|----------|-------------|-------------:|
| Bundled CLI | on-disk `dist` binary | 857,536 |
| Native extension | uncompressed wheel member | 5,131,760 |
| Wheel | `.whl` file on disk | 2,314,426 |

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
benchmark. The required functional suites additionally cover ranges, large
file streams, connection admission, timeouts, TLS, and shutdown.

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
