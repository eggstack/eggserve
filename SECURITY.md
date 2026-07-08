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
