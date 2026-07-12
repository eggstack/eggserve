# Release Checklist

## Pre-release (every release)

- [ ] Version numbers synchronized across all crates and Python package
- [ ] CHANGELOG updated (if one exists)
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings` passes
- [ ] `cargo test -p eggserve-bin --features tls` passes
- [ ] Platform CI green (Linux, macOS, Windows)
- [ ] Python API unit tests pass (`PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v`)
- [ ] Python wheel smoke test passes
- [ ] Installed-wheel packaging smoke tests pass (`crates/eggserve-python/packaging-tests/run_all.sh`)
- [ ] `cargo audit` clean or exceptions documented
- [ ] `cargo deny check` clean or exceptions documented
- [ ] Dependency/license review complete
- [ ] README examples manually verified
- [ ] Security policy reviewed
- [ ] Known limitations documented
- [ ] No accidental broad feature claims in docs or README

## CI and release hardening (Plans 039–040)

- [ ] All GitHub Actions pinned to SHA digests (see `.github/workflows/` for pinned versions)
- [ ] Workflow permissions are minimal (`contents: read` for CI, `contents: write` only for release)
- [ ] Supply-chain audit job passes (`cargo audit` + `cargo deny check` in CI)
- [ ] Raw-wire correctness tests pass in CI
- [ ] Corpus replay tests pass in CI
- [ ] Multi-OS CI matrix passes (Linux, macOS, Windows)
- [ ] Release workflow tested in dry-run mode before first real release

## For crates.io publication (if applicable)

- [ ] `cargo package -p eggserve-core --locked` passes
- [ ] `cargo build -p eggserve-bin --locked` passes before core is published; `cargo publish -p eggserve-bin --locked` performs package verification after the core crate is available
- [ ] Package metadata (description, license, repository) complete in Cargo.toml
- [ ] README renders correctly on crates.io

## For PyPI publication (if applicable)

- [ ] `maturin build --release -o dist` succeeds
- [ ] `python -m twine check dist/*` passes (if twine available)
- [ ] Wheel installs cleanly: `pip install --force-reinstall dist/*.whl`
- [ ] `python -m eggserve --help` works
- [ ] `dist/` outputs are NOT committed to source control (`.gitignore` excludes them)

## Release notes

- [ ] Release notes do not claim production-readiness
- [ ] Alpha/beta limitations are clearly stated
- [ ] Known limitations are listed
- [ ] Supported platforms are documented

## Release blockers

The following items block specific release milestones:

### Blocks 1.0

- Stable public API not frozen
- Signed releases not implemented

### Blocks Windows production claims

- Windows is explicitly a trusted/local-use platform (parser-level checks only)
- Reparse-point/NTFS junction hardening is a documented non-goal — see `docs/non-goals.md`
- Release notes must state: "Do not use with untrusted mutable public content on Windows"

### Blocks follow-symlinks production claims

- Follow-symlinks uses canonicalize-based resolution; not covered by the descriptor-relative hardening guarantee. Release notes must mark it explicitly as weaker/experimental.

### Not blockers (documented non-features)

- Multi-range requests absent — single byte ranges are supported
- HTTP/2 absent — documented limitation
- No native TLS unless `tls` feature enabled — documented
