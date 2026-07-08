# Release Criteria

## Alpha

An alpha release requires:

- [x] Functional CLI: `eggserve [DIR]` serves static files over HTTP
- [x] Safe defaults enforced: loopback bind, no symlinks, no dotfiles, no directory listing
- [x] Basic path regression tests: traversal attempts are denied
- [x] Workspace builds: `cargo build --workspace` succeeds
- [ ] Documentation complete: all docs in `docs/` are written and accurate
- [ ] Lint clean: `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] Format clean: `cargo fmt --all -- --check` passes

## Beta

A beta release requires all alpha criteria plus:

- [ ] Fuzz targets: path resolution and request parsing have fuzz coverage
- [ ] Multi-platform CI: Linux, macOS, and Windows builds pass in CI
- [x] Resource-limit tests: connection limits, file-stream limits, request body rejection, and timeouts are tested
- [ ] Dependency audit: `cargo audit` and `cargo deny` pass with no advisories
- [ ] No known unsound `unsafe` code

## 1.0

A 1.0 release requires all beta criteria plus:

- [ ] Dependency audit clean: `cargo audit` and `cargo deny` pass with no advisories or unresolved warnings
- [ ] Documented security review: a written review of the threat model and defensive layers
- [ ] Windows path coverage: Windows-specific path edge cases (UNC, `\\?\`, drive letters) are tested
- [ ] Stable public API: `eggserve-core` public API is reviewed and frozen for the 1.x series
- [ ] Signed releases: release artifacts are signed
- [ ] No outstanding security issues in the issue tracker
