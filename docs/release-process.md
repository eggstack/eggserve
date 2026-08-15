# Release Process

Manual wheel smoke tests use a temporary directory containing a deterministic
fixture file, pass the explicit directory and loopback bind to the installed
extension-backed CLI, poll readiness with bounded retries, verify the exact response body, and
confirm clean termination. Distribution artifacts use the workspace `dist`
profile; size comparisons distinguish profile/stripping effects from code
changes.

eggserve releases are performed manually by a maintainer from a trusted local
environment. GitHub Actions never publishes to crates.io, PyPI, or GitHub
Releases. The release cadence is a maintainer decision and is not triggered by
merges, pushes, tags, or CI state.

Historical plans (039, 044–046, 086, 089, 090) defined an evidence-driven
release framework with gate registries, generated checklists, and automated
publication. The current release workflow supersedes the earlier infrastructure
requirements while
preserving the product implementation and test coverage they created.

## Pre-release verification

```sh
git status --short                           # must be empty
./scripts/verify.sh full                     # or at minimum: cargo fmt/check/test + features
```

## Distribution builds

The `dist` profile produces stripped, size-optimized release artifacts for
distribution. Use it for manual release builds only — not for routine CI or
development:

```sh
cargo build --profile dist --locked -p eggserve-bin              # default CLI (no TLS)
cargo build --profile dist --locked -p eggserve-bin --features tls  # TLS CLI
```

The dist profile uses `opt-level = "z"`, fat LTO, single codegen unit,
and symbol stripping. See `Cargo.toml` for the exact configuration.

For Python wheels, build the extension-backed artifact directly through Maturin:

```sh
cd crates/eggserve-python
maturin build --profile dist --interpreter python -o dist
```

## crates.io publication

Core crate must be published before the binary crate, because the binary
depends on it by path (registry resolves the latest published version).

```sh
cargo publish -p eggserve-core --locked --dry-run
cargo publish -p eggserve-core --locked

# Wait for the new version to appear on the crates.io index.

cargo publish -p eggserve-bin --locked --dry-run
cargo publish -p eggserve-bin --locked
```

Versions are immutable on crates.io. If a version has been successfully
published and needs correction, a new version number is required. Do not retry
publication of changed contents under an existing version.

## Python artifact build and manual publication

Build the platform wheel directly from the Python crate; the wheel contains no
second standalone CLI executable:

```sh
cd crates/eggserve-python
maturin build --profile dist --interpreter python -o dist
```

Upload to PyPI manually:

```sh
pip install twine
twine upload dist/*.whl
```

GitHub Actions only builds and uploads wheel artifacts; it has no publication
credentials or publish job. A maintainer may upload a reviewed wheel manually
with `twine`. Python publication is independent of crates.io publication and
is not required to happen in the same transaction.

## Post-publication

After publication, optionally create a tag:

```sh
git tag "vX.Y.Z"
git push origin "vX.Y.Z"
```

The tag is a historical marker only. A GitHub Release may be created manually
if desired.

Run post-publication smoke tests:

```sh
pip install eggserve
eggserve --help
python -m eggserve --help
```

## Known limitations

- **Windows**: functionally qualified on the manual platform workflow, but not
  hardened for untrusted content. Two open-descendant root-rename cases are
  explicitly skipped because NTFS rejects that external path operation.
- **Follow-symlinks**: weaker than default symlink-denied mode. Uses
  canonicalize-based resolution outside the descriptor-relative hardening
  guarantee.
- **HTTP/2, redirects, retries, cookies, proxy, and multi-range responses**:
  outside scope. HTTP/1.1 with single byte ranges only.
- **Python wheels**: CPython 3.11+ with abi3 stable ABI (`>=3.11`).
