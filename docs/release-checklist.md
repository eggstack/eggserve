# Release Checklist

## Pre-release (every release)

- [ ] Version numbers synchronized across all crates and Python package
- [ ] CHANGELOG updated (if one exists)
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo check --workspace --features tls` passes
- [ ] Platform CI green (Linux, macOS, Windows)
- [ ] Python wheel smoke test passes
- [ ] `cargo audit` clean or exceptions documented
- [ ] Dependency/license review complete
- [ ] README examples manually verified
- [ ] Security policy reviewed
- [ ] Known limitations documented
- [ ] No accidental broad feature claims in docs or README

## For crates.io publication (if applicable)

- [ ] `cargo package -p eggserve-core --allow-dirty` passes
- [ ] `cargo package -p eggserve-bin --allow-dirty` passes
- [ ] Package metadata (description, license, repository) complete in Cargo.toml
- [ ] README renders correctly on crates.io

## For PyPI publication (if applicable)

- [ ] `maturin build --release -o dist` succeeds
- [ ] `python -m twine check dist/*` passes (if twine available)
- [ ] Wheel installs cleanly: `pip install --force-reinstall dist/*.whl`
- [ ] `python -m eggserve --help` works

## Release notes

- [ ] Release notes do not claim production-readiness
- [ ] Alpha/beta limitations are clearly stated
- [ ] Known limitations are listed
- [ ] Supported platforms are documented

## Release blockers

The following items block specific release milestones:

### Blocks 1.0

- Descriptor-relative/openat traversal not complete
- Windows reparse-point policy not complete
- Stable public API not frozen
- Signed releases not implemented

### Blocks Windows production claims

- Windows reparse-point behavior not audited/tested
- Windows-specific path edge cases not fully covered

### Not blockers (documented non-features)

- Range requests absent — documented limitation
- HTTP/2 absent — documented limitation
- No native TLS unless `tls` feature enabled — documented
