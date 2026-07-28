# Release Criteria

> **Historical note:** The machine-readable release criteria system
> (`release/criteria.toml`, `scripts/release_criteria.py`,
> `scripts/release-validate.sh`, `docs/release-checklist.md`) has been
> superseded by Plan 091, which establishes a manual crates.io release
> procedure documented in `docs/release-process.md`. The sections below are
> retained as historical reference.

## Alpha (historical)

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

## Beta (historical)

- [x] Fuzz targets: path resolution and request parsing have fuzz coverage
- [ ] Multi-platform CI: Linux, macOS, and Windows builds pass in CI
- [x] Resource-limit tests: connection limits, file-stream limits, request body rejection, and timeouts are tested
- [ ] Dependency audit: `cargo audit` passes with documented exceptions
- [x] deny.toml present for automated license/advisory checking
- [x] Platform support matrix documented
- [x] Security review note documented
- [x] TLS feature validated in CI: clippy and tests with `--features tls`
- [ ] Python API tests run in CI from source and installed wheels pass on Linux, macOS, and Windows
- [ ] `cargo deny check` runs in CI as a release gate
- [x] Filesystem denial taxonomy is meaningful
- [x] Python `ServeConfig` validates port, log format, and public-bind combinations at construction
- [x] TLS handshakes are bounded by `--header-timeout`
- [x] `eggserve-core` public API surface is documented: stable-ish, experimental, internal
- [ ] Supply-chain audit job in CI: pinned `cargo audit` + `cargo deny check`
- [ ] Raw-wire correctness tests in CI
- [ ] Corpus replay in CI
- [x] GitHub Actions pinned to SHA digests
- [x] Workflow permissions minimal
- [ ] No known unsound `unsafe` code

## 1.0 (historical)

- [ ] Dependency audit clean: `cargo audit` and `cargo deny` pass with no advisories or unresolved warnings
- [ ] Documented security review: a written review of the threat model and defensive layers
- [ ] Windows path coverage: Windows-specific path edge cases (UNC, `\\?\`, drive letters) are tested
- [ ] Windows reparse-point coverage: reparse-point/junction hardening is audited and tested
- [ ] Stable public API: `eggserve-core` public API is reviewed and frozen for the 1.x series
- [ ] Signed releases: release artifacts are signed
- [ ] No outstanding security issues in the issue tracker
- [x] Descriptor-relative traversal: filesystem traversal uses directory-fd/`openat`-style resolution on Unix with safe defaults
