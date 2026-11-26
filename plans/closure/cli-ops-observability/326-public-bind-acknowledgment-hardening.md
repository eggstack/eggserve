# Closure — Plan 326 public-bind acknowledgment hardening

Status: closed

Implementation: `plans/implementation/cli-ops-observability/326-public-bind-acknowledgment-hardening.md`

## What landed

- CLI `Args` rejects any resolved non-loopback bind without `--public`
  (`!bind_ip.is_loopback()`), not only unspecified addresses.
- Python `ServeConfig` and the native lowlevel constructor use the same
  non-loopback gate.
- `HTTPServer` address normalization treats non-loopback IP literals and
  hostnames as explicit public intent (Plan 094 constructor-as-intent);
  loopback literals stay quiet. `--public` / `public=True` remains
  acknowledgment only and does not expand the host.
- Docs corrected: `docs/cli.md`, `docs/security-policy.md`,
  `docs/deployment.md`, `docs/security-review.md`,
  `docs/release-contract.md`, `architecture/eggserve-bin.md`,
  `architecture/policy-system.md`.

## Evidence

```text
cargo test -p eggserve-bin --lib public_
  → 5 passed (includes non_loopback_unicast_* and loopback_bind_*)

PYTHONPATH=python:tests python3 -m unittest \
  test_server.TestConfigValidation.test_non_loopback_unicast_without_public_raises \
  test_server.TestConfigValidation.test_non_loopback_unicast_with_public_true_allowed \
  test_server.TestConfigValidation.test_loopback_bind_does_not_require_public \
  test_server.TestConfigValidation.test_public_ipv4_without_public_raises \
  test_server.TestLegacyPublicBindGuard
  → 13 passed
```

## Residuals

None for this gate. Accepted filesystem/protocol residuals from the security
audit are unchanged and out of scope for 326.
