# Plan 326 — Public-bind acknowledgment hardening

Status: closed

Source roadmap: `plans/subsystems/cli-ops-observability-roadmap.md`

## Objective

Close the gap between the threat-model asset (“without `--public`, do not
bind a public interface”) and the CLI/Python gate that only rejected
*unspecified* addresses (`0.0.0.0` / `::`). Require an explicit public
acknowledgment for any non-loopback bind on the CLI-facing surfaces, while
preserving the Python `http.server` contract that an explicit constructor
address is itself the acknowledgment.

## Non-goals

- Do not make `--public` alone expand the default bind to `0.0.0.0`
  (Plan 303 verified `--public --port` still binds loopback).
- Do not add authentication, per-IP rate limiting, or bind-interface
  allowlists.
- Do not change filesystem confinement, TLS, or protocol tiers.

## Current evidence

- `crates/eggserve-bin/src/args.rs` rejects only `bind_ip.is_unspecified()`.
- `docs/threat-model.md` names any public interface, not only wildcards.
- `docs/deployment.md` / `docs/security-policy.md` incorrectly imply
  `--public` alone binds `0.0.0.0`; Plan 303 evidence shows acknowledgment
  without address expansion.
- Python `ServeConfig` mirrors the unspecified-only check; native
  `eggserve-python` runtime does the same; `HTTPServer` auto-sets
  `public` only for wildcards.

## Work packages

1. CLI: reject `!public && !bind_ip.is_loopback()`; update help text and
   unit tests (unicast non-loopback without `--public`, allowed with it,
   loopback still free).
2. Python `ServeConfig` + native lowlevel constructor: same non-loopback
   gate; `HTTPServer` normalize treats non-loopback IP literals and
   hostnames as explicit public intent (loopback literals stay quiet).
3. Docs: `docs/cli.md`, `docs/security-policy.md`, `docs/deployment.md`,
   `architecture/eggserve-bin.md`, `architecture/policy-system.md` —
   describe acknowledgment for any non-loopback bind; correct the false
   “`--public` alone binds 0.0.0.0” claim.
4. Register milestone 326 on the CLI roadmap + `plans/registry.md`.

## Verification

```sh
cargo test -p eggserve-bin --lib
# focused Python when wheel env available:
# python -m unittest crates.eggserve-python.tests.test_server
```

## Acceptance

- Non-loopback CLI bind without `--public` fails.
- Loopback and wildcard-with-`--public` still work.
- Docs no longer claim `--public` alone expands the bind address.
