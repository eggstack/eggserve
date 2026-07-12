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

eggserve is a static file server. Security issues in upstream dependencies (Rust, hyper, tokio, etc.) should be reported to the respective projects. Report to eggserve only if the vulnerability is in eggserve's own code or policy enforcement.

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
