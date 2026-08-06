# Binary Size Tracking

This is the Plan 109 corrective-pass measurement snapshot. Sizes are recorded
separately for the unstripped `release` profile and the stripped `dist`
distribution profile; those profiles must not be compared as if the difference
were solely code-size change.

## Environment

- Target: `x86_64-unknown-linux-gnu`
- Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Candidate SHA: `cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d` (`fix: close final admission and wire verification gaps`)
- Maturin: 1.14.1

## CLI artifacts

| Artifact | Profile | Stripped | Size (bytes) |
|----------|---------|----------|-------------:|
| Default CLI | release | no | 1,966,408 |
| Default CLI | dist | yes | 856,920 |
| TLS CLI | release | no | 3,075,040 |
| TLS CLI | dist | yes | 1,218,048 |

The `dist` profile is approximately 56.4% smaller than this snapshot's
unstripped default release artifact and 60.4% smaller for TLS. The comparison
is profile-aware: `dist` uses size optimization, single-unit LTO, one codegen
unit, and symbol stripping.

## Python wheel artifacts

Measured from the CPython 3.14 Linux wheel built by
`scripts/test-python-wheel.sh`:

| Artifact | Measurement | Size (bytes) |
|----------|-------------|-------------:|
| Bundled CLI | uncompressed wheel member (default `dist`) | 856,920 |
| Native extension | uncompressed wheel member | 2,255,752 |
| Wheel | `.whl` file on disk | 1,573,717 |

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
benchmark. No current-thread versus multi-thread performance comparison was
performed; the standalone CLI has only the current-thread production variant.
Embedded current-thread and multi-thread lifecycle coverage remains functional
coverage, not a performance result. Large-file and range correctness are
covered by the streaming and wire suites rather than treated as a permanent
performance gate.

The measured default `dist` CLI has SHA-256
`f7b69951e629796672073bc110f7f968d8479d482b3a578bac2f69a1eeb669b9`; the
TLS `dist` CLI has SHA-256
`9aa1a5ece3b2ae3bce9aaaf59822e3c88e9fffbcf2fe37d7b8fd2a8e1c4033e4`. The
Linux CPython 3.14 wheel has SHA-256
`8502e5e8f4961920a40f1d13955d7cfc75a7bac797033ec169da0c222ac40d40`.

## Reproduction commands

Because the default and TLS builds produce the same target filenames
(`target/release/eggserve` and `target/dist/eggserve`), the later TLS build
overwrites the earlier default build. Each variant must be captured immediately
after its build into a unique path.

### Clean-state preparation

Remove stale artifacts that could contaminate measurements:

```sh
rm -rf target/release target/dist
rm -rf crates/eggserve-python/target
rm -rf crates/eggserve-python/python/eggserve/bin
rm -rf dist
```

### Build and capture each variant

```sh
artifact_dir="$(mktemp -d)"

cargo build --release --locked -p eggserve-bin
cp target/release/eggserve "$artifact_dir/eggserve-default-release"

cargo build --profile dist --locked -p eggserve-bin
cp target/dist/eggserve "$artifact_dir/eggserve-default-dist"

cargo build --release --locked -p eggserve-bin --features tls
cp target/release/eggserve "$artifact_dir/eggserve-tls-release"

cargo build --profile dist --locked -p eggserve-bin --features tls
cp target/dist/eggserve "$artifact_dir/eggserve-tls-dist"
```

### Measure unique captured artifacts

```sh
stat --printf='%n %s\n' "$artifact_dir"/eggserve-*
sha256sum "$artifact_dir"/eggserve-*
```

### Verify packaged CLI identity

After the supported wheel script builds and stages the default non-TLS `dist`
CLI, verify SHA-256 equality among the unique capture, the staged binary, and
the wheel-extracted member:

```sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh

sha256sum \
  "$artifact_dir/eggserve-default-dist" \
  crates/eggserve-python/python/eggserve/bin/eggserve

# Extract the bundled CLI from the wheel for comparison
python3.14 -c "
import zipfile, glob, hashlib, sys
whl = glob.glob('dist/eggserve-*.whl')[0]
member = [n for n in zipfile.ZipFile(whl).namelist() if n.endswith('eggserve')][0]
data = zipfile.ZipFile(whl).read(member)
open('/tmp/eggserve-wheel-extracted', 'wb').write(data)
print(hashlib.sha256(data).hexdigest())
"

sha256sum \
  "$artifact_dir/eggserve-default-dist" \
  crates/eggserve-python/python/eggserve/bin/eggserve \
  /tmp/eggserve-wheel-extracted
```

All three must match. The recorded snapshot itself is Linux
`x86_64-unknown-linux-gnu`.

```sh
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-core --no-default-features
```

The wheel script uses a controlled temporary fixture for its bundled CLI
smoke, so release verification does not depend on a repository directory
listing.
