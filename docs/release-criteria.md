# Release Criteria

## Alpha

An alpha release requires:

- [x] Functional CLI: `eggserve [DIR]` serves static files over HTTP
- [x] Safe defaults enforced: loopback bind, no symlinks, no dotfiles, no directory listing
- [x] Basic path regression tests: traversal attempts are denied
- [x] Workspace builds: `cargo build --workspace` succeeds
- [x] Documentation complete: all docs in `docs/` are written and accurate
- [x] Lint clean: `cargo clippy --workspace --all-targets -- -D warnings` passes
- [x] Format clean: `cargo fmt --all -- --check` passes
- [x] Listing policy: directory listings respect symlink-denied policy
- [x] Error taxonomy: denial reasons distinguish symlink/root-escape from parent traversal
- [x] Body metadata: GET and HEAD test coverage is symmetrical for Content-Length/Transfer-Encoding

## Beta

A beta release requires all alpha criteria plus:

- [x] Fuzz targets: path resolution and request parsing have fuzz coverage
- [x] Multi-platform CI: Linux, macOS, and Windows builds pass in CI
- [x] Resource-limit tests: connection limits, file-stream limits, request body rejection, and timeouts are tested
- [x] Dependency audit: `cargo audit` passes with no advisories
- [ ] No known unsound `unsafe` code

## 1.0

A 1.0 release requires all beta criteria plus:

- [ ] Dependency audit clean: `cargo audit` and `cargo deny` pass with no advisories or unresolved warnings
- [ ] Documented security review: a written review of the threat model and defensive layers
- [ ] Windows path coverage: Windows-specific path edge cases (UNC, `\\?\`, drive letters) are tested
- [ ] Stable public API: `eggserve-core` public API is reviewed and frozen for the 1.x series
- [ ] Signed releases: release artifacts are signed
- [ ] No outstanding security issues in the issue tracker
- [ ] Descriptor-relative traversal: filesystem traversal uses directory-fd/`openat`-style resolution on Unix (or an explicitly documented alternative approved at the 1.0 review) so that component-wise symlink checks cannot be bypassed by TOCTOU between metadata inspection and file open. The current alpha implementation is component-wise metadata + canonical-root verification only.
