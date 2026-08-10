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

## Plan 114 — Dependency and Artifact Slimming

Plan 114 removed unused direct dependencies from `eggserve-bin` (`hyper`,
`hyper-util`, `http-body-util`, `bytes`) and from `eggserve-python` (`hyper`,
`hyper-util`, `http-body-util`, `bytes`, `futures-util`). These were manifest
declarations that no source code imported; the actual linked code was unchanged.
Artifact sizes before and after are identical within compiler noise:

| Artifact | Before (bytes) | After (bytes) |
|----------|---------------:|--------------:|
| Default CLI (dist) | 856,920 | 856,920 |
| TLS CLI (dist) | 1,218,048 | 1,218,032 |

The TLS delta (−16 bytes) is within ordinary linker noise. The primary outcome
is manifest correctness: every remaining direct dependency has an active
source-level reason.

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

`scripts/test-python-wheel.sh` is the supported installed-wheel verification
harness. It intentionally removes the staged CLI and temporary wheel directory
on exit; do not hash files after the script has deleted them.

To verify packaged CLI identity, rebuild and stage the default non-TLS `dist`
CLI separately, build the wheel into a persistent temporary directory, and
extract the bundled CLI before comparing SHA-256 hashes:

```sh
set -euo pipefail

# Rebuild the default non-TLS dist CLI (the TLS build overwrote target/dist/eggserve)
cargo build --profile dist --locked -p eggserve-bin
stage_dir="crates/eggserve-python/python/eggserve/bin"
mkdir -p "$stage_dir"
cp target/dist/eggserve "$stage_dir/eggserve"
chmod +x "$stage_dir/eggserve"

# Build the wheel into a persistent temporary directory
wheel_dir="$(mktemp -d)"
(
  cd crates/eggserve-python
  python3.14 -m maturin build \
    --profile dist \
    --interpreter python3.14 \
    -o "$wheel_dir"
)

# Extract the bundled CLI from the wheel
python3.14 - "$wheel_dir" "$artifact_dir/eggserve-wheel-extracted" <<'PY'
import pathlib
import sys
import zipfile

wheel_dir = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
wheel = next(wheel_dir.glob("eggserve-*.whl"))
with zipfile.ZipFile(wheel) as zf:
    members = [
        name for name in zf.namelist()
        if name.endswith("/eggserve") or name.endswith("/eggserve.exe")
    ]
    if len(members) != 1:
        raise SystemExit(f"expected one bundled CLI, found {members!r}")
    out.write_bytes(zf.read(members[0]))
PY

# Compare hashes: default-dist capture, staged CLI, wheel-extracted CLI
sha256sum \
  "$artifact_dir/eggserve-default-dist" \
  "$stage_dir/eggserve" \
  "$artifact_dir/eggserve-wheel-extracted"
```

All three must match. The recorded snapshot itself is Linux
`x86_64-unknown-linux-gnu`.

`scripts/test-python-wheel.sh` remains the supported installed-wheel
verification harness. The manual capture recipe above exists specifically to
preserve artifacts long enough for reproducibility and hash comparison; running
it does not replace the installed-wheel verification.

```sh
cargo tree -e features -p eggserve-bin --no-default-features
cargo tree -e features -p eggserve-core --no-default-features
```

The wheel script uses a controlled temporary fixture for its bundled CLI
smoke, so release verification does not depend on a repository directory
listing.
