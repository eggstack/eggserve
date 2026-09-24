# Plan 285 — Embedding contract qualification

## Local source qualification

- Caller-owned Rustls/Tokio-Rustls H1 test negotiates ALPN `http/1.1` on both
  ends and passes the established server TLS stream directly into
  `serve_http1_connection_with_policy`.
- Default direct H1 behavior and service/tunnel bounded admission retain their
  existing suites. External handler/body/idle/write deadlines and body/target
  ceilings are exercised in `downstream_embedding.rs`.
- Presenter integration checks status, safe custom metadata/body, and runtime
  framing. Presenter panic and bounded fallback are covered by unit tests.
- Tunnel direct transport passes exact read-ahead, upgrade/CONNECT,
  cancellation, shutdown, and admission tests.
- Direct tree (`cargo tree -p eggserve-server --no-default-features -e
  no-dev`) contains no `eggserve-core`, `eggserve-static`, or PHF ancestry.
  The optional Tower suite passes with `cargo test -p eggserve-server
  --features tower`.

## API and version decision

Select **0.3.0** for the direct server line: `RuntimeConfig` is public with
public fields, and the existing `RuntimeState::service_semaphore` and
`tunnel_semaphore` accessors now return `Option` to represent external
ownership truthfully. Those are source-incompatible changes for exhaustive
struct literals and direct accessor consumers. Plan 286 must derive exact
artifact versions and package set from the live registry baseline.

`cargo semver-checks 0.49.0` compared changed publish packages to their latest
published baselines: primitives 0.2.0 -> 0.2.1 and bin 0.2.0 -> 0.2.1 had
196 compatible checks pass each; server 0.2.1 -> 0.3.0, core 0.2.2 -> 0.3.0,
static 0.2.0 -> 0.3.0, and H3 0.2.0 -> 0.3.0 were explicitly classified as
major changes (the tool skips fine-grained checks across major releases).

## Local verification

- `cargo fmt --all -- --check`
- `cargo +1.89 check --workspace --all-targets`
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`
- `cargo test --workspace` (passed after explicit ring CryptoProvider
  selection in the local TLS fixture)
- `python3 scripts/check-crate-topology.py`
- `python3 scripts/verify-conformance-matrix.py`
- `python3 scripts/check-python-release-metadata.py`
- `bash scripts/install-cargo-tools.sh`
- `bash scripts/check-supply-chain.sh` (both lockfiles passed)
- `bash scripts/verify-cargo-packages.sh --mode all` (staged package graph
  and registry-local direct/core Tower consumers passed)
- `cargo publish -p eggserve-primitives --locked --dry-run --allow-dirty`

Hosted CI and proof-bearing SHA are pending the pushed candidate.

The staged Tower package consumers reported 45 direct lockfile packages,
111 direct no-dev dependency nodes, and a 579,576 byte release executable;
the core compatibility consumer reported 62 lockfile packages, 155 no-dev
nodes, and a 647,096 byte release executable. These are package-graph
measurements, not general performance claims.

## Residual qualification limits

Plan 284's A/B harness is deterministic in-process Tokio duplex rather than
TCP loopback; it does not isolate process CPU or RSS. Its narrow mechanism
evidence and limits are recorded in `plan-284-tunnel-transport-ab-qualification.md`.
Cross-platform qualification remains a hosted/manual release gate.
