# Security Policy

## Reporting vulnerabilities

If you discover a security vulnerability in eggserve, please report it responsibly:

- **Email:** dbowman91@proton.me
- **GitHub Private Advisory:** Use the repository's private vulnerability reporting feature

Do not open public issues for security vulnerabilities.

## Supported versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | Yes (early alpha)  |
| < 0.1   | No                 |

## Security policy

The full security policy, including safe defaults and threat model, is documented in:

- [docs/security-policy.md](docs/security-policy.md) — safe defaults and opt-in behaviors
- [docs/threat-model.md](docs/threat-model.md) — assets, trust boundaries, and attacker capabilities

## Scope

EggServe is primarily a hardened static-file server, but its security boundary
also includes the reusable HTTP runtime and Python facade. Report issues in
the implementation or policy of these surfaces, including:

- HTTP/1.1 and optional HTTP/2 parsing, request/response framing, admission,
  lifecycle, timeout, and resource-limit behavior;
- experimental HTTP/3/QUIC stream handling and its bounded integration with the
  canonical service pipeline;
- TLS identity selection, SNI, ALPN, optional/required client authentication,
  operator-supplied trust stores and CRLs, certificate-chain exposure, and
  atomic TLS configuration reload;
- explicitly configured trusted forwarding and PROXY protocol metadata,
  listener ownership/adoption, and connection admission;
- tunnel/upgrade handoff, custom Rust `Service` implementations at the
  EggServe boundary, and bounded Python handlers/callbacks; and
- static-file path validation, descriptor/handle-relative confinement,
  symlink/dotfile/listing policy, response normalization, and sanitized
  operational output.

An upstream defect in Hyper, Rustls, Quinn, H3, Tokio, PyO3, or another
dependency is reportable to that project, but also report it to EggServe when
it can affect an EggServe-supported or documented experimental configuration,
when EggServe enables the vulnerable behavior, or when our version policy,
feature gating, validation, or response to the advisory is inadequate. Do not
assume that an upstream report alone establishes that EggServe is unaffected.
Include the dependency version, enabled feature set, protocol/profile, and a
minimal reproduction when safe to share.

The release wheel targets the CPython 3.11 stable ABI (abi3) and supports
GIL-enabled CPython 3.11+. Routine CI verifies the Linux wheel with CPython
3.14; macOS and Windows wheels are built and tested in the release/platform
workflows. Windows implements handle-relative confinement for the qualified
classes, but remains trusted/local-content only: two open-descendant
root-rename cases are skipped because NTFS rejects that external path
operation. This is a qualification limitation, not an absence of confinement.

EggServe denies application-owned Rust `unsafe_code` by default. The reviewed
exceptions are documented in [docs/unsafe-code-policy.md](docs/unsafe-code-policy.md).

## Vulnerability triage

1. Acknowledge receipt within 48 hours.
2. Assess severity using CVSS or equivalent scoring.
3. Determine affected versions and fix timeline.
4. Prepare a patch on a private branch (not `main`).

## Embargo policy

Confirmed vulnerabilities with severity above moderate are embargoed until a fix is available and released. During embargo:

- No public disclosure of the vulnerability details.
- No mention in public changelogs beyond "security fix".
- Fixes are developed on private branches.
- Maintainers notify the reporter before public release.

## Dependency advisory response

When `cargo audit` or GitHub advisory databases report a vulnerability:

1. Assess whether the affected dependency is actually reachable in eggserve's code paths.
2. If exploitable, treat as a vulnerability per the triage process above.
3. If not exploitable (unreachable code path, wrong feature gate, etc.), document the finding and rationale for accepting the risk.
4. Update `deny.toml` or `audit.toml` only with documented justification.

Every distributed Rust dependency closure is checked in routine CI and release
preflight. The root workspace and the excluded Python wheel crate have
separate lockfiles; `scripts/check-supply-chain.sh` audits both and applies the
shared `deny.toml` policy to both manifests. A release build also uses the
exact Rust 1.98.1 compiler rather than a floating stable patch version.

## Release revocation / yank procedure

If a released version contains a vulnerability or critical bug:

1. Assess severity and determine whether to yank or release a patch fix.
2. For crates.io: `cargo yank --version <version>` (requires registry token).
3. For PyPI: use the PyPI yank UI or API (requires maintainer access).
4. Create a GitHub Security Advisory if the vulnerability warrants CVE tracking.
5. Notify affected users through GitHub release notes.
6. Update `SECURITY.md` supported-versions table if prior versions become unsupported.

## Contact ownership

The primary maintainer is David Bowman (dbowman91@proton.me). Security-related communications should be directed to this address or through GitHub's private vulnerability reporting.
