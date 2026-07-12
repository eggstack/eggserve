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
- [x] Dependency audit: `cargo audit` passes with documented exceptions (see [dependency-policy.md](dependency-policy.md) for accepted `rustls-pemfile` unmaintained warning)
- [x] deny.toml present for automated license/advisory checking
- [x] Release checklist documented
- [x] Platform support matrix documented
- [x] Security review note documented
- [x] TLS feature validated in CI: clippy and tests with `--features tls`
- [x] Python API tests run in CI from source
- [x] `cargo deny check` runs in CI as a release gate
- [x] Filesystem denial taxonomy is meaningful: `ResolvedResource::Denied(PathRejection)` preserves the specific denial reason for tests, with HTTP responses still returning a generic 403
- [x] Python `ServeConfig` validates port, log format, and public-bind combinations at construction
- [x] TLS handshakes are bounded by `--header-timeout`
- [x] `eggserve-core` public API surface is documented: stable-ish, experimental, internal
- [x] Supply-chain audit job in CI: `cargo audit` + `cargo deny check` run on every push and PR
- [x] Raw-wire correctness tests in CI: `http_wire_correctness`, `http_primitives_integration`, `production_path` run in CI
- [x] Corpus replay in CI: fuzz corpus replay runs on every push and PR
- [x] GitHub Actions pinned to SHA digests: all third-party actions use immutable commit references
- [x] Workflow permissions minimal: CI workflows declare `contents: read`, release workflow uses `contents: write` + `id-token: write`
- [ ] No known unsound `unsafe` code

## 1.0

A 1.0 release requires all beta criteria plus:

- [ ] Dependency audit clean: `cargo audit` and `cargo deny` pass with no advisories or unresolved warnings
- [ ] Documented security review: a written review of the threat model and defensive layers
- [ ] Windows path coverage: Windows-specific path edge cases (UNC, `\\?\`, drive letters) are tested
- [ ] Windows reparse-point coverage: reparse-point/junction hardening is audited and tested
- [ ] Stable public API: `eggserve-core` public API is reviewed and frozen for the 1.x series
- [ ] Signed releases: release artifacts are signed
- [ ] No outstanding security issues in the issue tracker
- [x] Descriptor-relative traversal: filesystem traversal uses directory-fd/`openat`-style resolution on Unix with safe defaults (symlinks denied). Each component is checked with `statat(AT_SYMLINK_NOFOLLOW)` and opened with `openat(O_NOFOLLOW)`; if a symlink is swapped into place between the two, the open fails rather than following. Falls back to canonicalize-based resolution on non-Unix or when `--follow-symlinks` is enabled; follow-symlinks is explicitly outside the descriptor-relative hardening guarantee.
