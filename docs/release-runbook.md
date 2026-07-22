# Release Operator Runbook

This runbook covers the end-to-end process for producing an eggserve release. Every step is traceable to a plan in `plans/` and a gate in `release/criteria.toml`. No manual step may silently mark a missing required gate as passed.

## Prerequisites

- Plans 075–089 are closed with exact-SHA evidence.
- All required CI gates pass on the candidate SHA.
- No open high/critical findings from independent review.

## 1. Freeze the Release Candidate

1. Select the release-candidate commit.
2. Record the exact SHA, version, Cargo.lock hash, Rust toolchain, Python/maturin toolchain, generated-file state, support-profile metadata hash, and release-criteria hash.
3. Verify clean tree: `git status --porcelain` must be empty.
4. Push the SHA and confirm CI passes all gate jobs.

## 2. Execute Required Workflows

Run the full CI pipeline. The following gate groups must pass:

### Preflight
- `rust.format`, `rust.clippy`, `rust.test`, `rust.doctest`
- `rust.test.client`, `rust.test.client-tls`, `rust.test.server-tls`
- `supply-chain.audit`, `supply-chain.deny`
- `check-generated`

### Qualification
- `http.raw-wire`, `http.production-path`, `http.primitives-integration`
- `filesystem.corpus-replay`, `filesystem.unix-race`
- `conformance.*` gates (canonical HTTP, wire interop, API inventory, perf regression)
- `python.*` gates (unit tests, native, server primitives, API stability, boundary hardening, lifecycle, parity, contract consistency)
- `body.*` gates (fixed-length, chunked, limit, timeout, partial, static rejection, corpus, wire, TLS parity)
- `runtime.*` gates (public consumer, service dispatch, listener lifecycle, graceful/forced shutdown)
- `pinned-root.*` gates (identity, no-reopen, descriptor leak, root replacement)
- `ops.*` gates (JSON log, text sanitization, library silence, backoff, streaming errors, log sink, observer parity, installed artifact logging)

### Plan 089 Qualification
- `proxy.caddy-interop`, `proxy.nginx-interop`, `proxy.desync-corpus`
- `native-tls.abuse-limits`
- `stateful.fuzz-replay`
- `fault.injection`
- `soak.unix-reverse-proxy` (and `soak.unix-direct-https` if promoting that profile)

### Artifact
- `package.core`, `package.bin`
- `python.wheel.linux`, `python.wheel.macos`, `python.wheel.windows`
- `python.packaging-smoke`
- `artifact.installed-binaries`
- `supply-chain.sbom`

### Windows (if applicable)
- `windows.*` gates (handle-retained-directory, handle-relative-child, index, unicode, ownership, hardened-no-fallback, enumeration, buffer-parser, listing-*, reparse-matrix, namespace-matrix, race-root-escape, root-identity, validator-identity, resource-stability, installed-artifact, fuzz-corpus-replay)
- `windows.independent-safety-review`
- `windows.profile-decision`

## 3. Verify Exact-SHA Evidence

```sh
python3 scripts/release_criteria.py aggregate \
  --criteria release/criteria.toml \
  --evidence <evidence-dir> \
  --sha <candidate-sha>
```

The aggregator fails closed for:
- Missing evidence
- Stale evidence
- Evidence from a different SHA
- Evidence from a source build instead of required artifact
- Skipped gates due to unavailable fixtures
- Incomplete evidence for a promoted platform
- Open blocking findings

## 4. Inspect Soak/Fuzz/Race Artifacts

Review:
- Soak test logs for monotonic resource growth, task accumulation, permit loss, or shutdown deadline violations.
- Fuzz replay results for any unresolved failures.
- Race suite results for outside-root bytes served or denied-object bytes served.
- Fault injection results for panics, tight loops, or false lifecycle completion.

## 5. Confirm Findings Closed

- All critical/high findings from independent review must be fixed and gates rerun.
- Medium findings must be fixed or the affected profile narrowed with explicit documentation.
- Low findings may be deferred with owner and rationale.
- No finding may be hidden by changing test expectations without protocol/security justification.

## 6. Verify Artifact Hashes and Provenance

```sh
# Verify checksums
sha256sum -c checksums-sha256.txt

# Verify provenance record
cat provenance.json
```

The provenance record must bind artifact hashes to the exact release-candidate SHA. Missing or mismatched provenance blocks release.

## 7. Update Support Profiles

Update `release/support-profiles.toml` with final profile statuses. Each profile is decided independently:

- **unix-reverse-proxy**: Promote to `supported-hardened` only if proxy interop, filesystem races, soak, artifacts, and review pass.
- **unix-direct-https**: Promote from `candidate` only if native TLS, direct internet runtime, soak, artifacts, and review pass.
- **windows-reverse-proxy**: Promote only if Plan 086 and common gates pass.
- **windows-direct-https**: Promote only if both Windows hardening and native TLS qualification pass.
- **local-development**: Retain based on ordinary cross-platform gates.
- **Functional/compatibility profiles**: Keep explicitly outside hardened claims.

## 8. Human Approval

Record a signed decision with:
- Reviewer identity
- Source SHA reviewed
- Scope of review
- Findings and dispositions
- Approval or rejection with rationale

## 9. Tag, Release, Publish Order

1. Tag the release-candidate SHA: `git tag v0.1.0`
2. Push the tag: `git push origin v0.1.0`
3. CI builds release artifacts (binaries, wheels)
4. Verify artifacts match checksums and provenance
5. Publish to crates.io (eggserve-core, eggserve-bin) — dry-run first
6. Publish to PyPI (Python wheel)
7. Create GitHub release with artifacts and provenance

## 10. Post-Release Smoke Tests

After publication:
- `pip install eggserve` in a clean environment
- `eggserve --help` works
- `python -m eggserve --help` works
- Serve a directory and verify basic GET/HEAD
- Verify TLS with test certificate (if applicable)

## 11. Rollback/Yank Procedure

If a critical issue is discovered post-release:
1. Yank the PyPI package: `twine upload --repository pypi --skip-existing`
2. Yank the crates.io package: `cargo yank --version 0.1.0`
3. Tag a fix release
4. Publish advisory if security-related

## 12. Security Advisory Process

For security vulnerabilities:
1. Do not disclose publicly until a fix is available.
2. Prepare a fix branch.
3. Document the vulnerability with CVSS scoring.
4. Coordinate disclosure with the fix release.
5. Publish advisory through GitHub Security Advisories.

## Stop Conditions

Do not release or promote an affected profile if:
- CI/evidence cannot prove exact-SHA identity
- A required platform or fixture is unavailable
- Soak shows monotonic resource growth
- Proxy/origin parsers disagree on framing
- A race serves outside-root content
- Native TLS admission is unbounded
- Installed artifact behavior differs from source
- Independent review is incomplete
- Any high/critical finding remains open
