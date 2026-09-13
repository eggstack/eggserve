# Plan 216 closure — Direct generic tunnel/upgrade parity

## Selected design (Option B: server-owned service capability context)

Evaluated against the plan's architectural gate:

- **Option A (neutral capability in primitives)** rejected as the sole
  placement: a one-shot acceptance that returns a handshake `Response`
  while staging transport state cannot live in a Hyper/Tokio-free crate
  without either naming transport types or forcing a neutral-IO
  re-abstraction of the Tokio duplex ergonomics downstream codecs use.
  The neutral pieces that *can* live there (`TunnelKind`,
  `ProtocolName`, `TunnelRequest`, `TunnelError`, bounds, classifiers,
  handshake validator) did move to `eggserve-primitives::tunnel`.
- **Option B (server-owned capability context)** selected: canonical
  `Request`/`RequestContext` stay transport-neutral (intent only,
  cloneable); transport capabilities arrive through the additive
  `Service::call_with_tunnel` parameter (direct) while compatibility
  keeps the `RequestContext::take_tunnel()` slot backed by a thin
  wrapper over the same server state machine. One native service
  abstraction per stack, no `Any` map, no second service model:
  ordinary services ignore the capability (default drops it) and deny
  with ordinary HTTP unchanged.
- **Option C (new crate)** rejected: the vocabulary/execution split maps
  cleanly onto the existing leaf/transport boundary; a new crate would
  be a dumping ground, not a reusable role.

Handlers own only IO (`FnOnce(TunnelIo)`); the request lifecycle is
captured in the closure when cancellation is needed. Handoff is observed
out-of-band (pipeline sidecar), never carried inside `Response`
(`is_tunnel()` is always `false`; the compatibility token is deleted).

## Ownership before → after

| Piece | Before | After |
|---|---|---|
| Intent vocabulary, bounds, classifiers, handshake validator | `eggserve-core::primitives::tunnel` (+ unexported orphan copy in `eggserve-primitives`) | `eggserve-primitives::tunnel` (Hyper/Tokio-free); core re-exports; orphan resolved |
| `HeaderBlock`, `Authority` | duplicated copies | core facades re-export the direct types (identical files) |
| Capability/state/handshake builder/bridge (`run_tunnel`) | `eggserve-core` only | `eggserve-server::tunnel` (single authority) |
| H1 detection/commitment/admission | core pipeline + `server/connection/tunnel.rs` | direct pipeline + compatibility pipeline through shared helpers + shared future; `server/connection/tunnel.rs` deleted |
| H2 Extended CONNECT | core-only | same capability/future via shared helpers (H2 glue stays until Plan 217) |
| H3 | core adapter + token | same capability/sidecar + own stream bridging (Plan 213 boundary unchanged) |
| Service contract | `call` only (direct had no tunnel) | additive `call_with_tunnel` (direct) / `take_tunnel` wrapper (compat); handlers `FnOnce(TunnelIo)` everywhere |

## Evidence

- `crates/eggserve-server/tests/tunnel_upgrade.rs` (new, 9 tests): echo,
  read-ahead exactness, denial (ordinary/malformed/body-bearing/ignored),
  handshake bounds, admission 503, shutdown cancel, CONNECT stays
  ordinary.
- `crates/eggserve-core/tests/tunnel_upgrade.rs` (9 tests incl.
  `tokio-tungstenite` interop, after-commit, budget recovery): green on
  default, `http2,tls`, and `http3,tls` graphs — proving compatibility
  H1 reaches the direct implementation.
- `scripts/check-crate-topology.py` Plan 216 gate: neutral-tunnel grep,
  server ownership markers, deleted core transport, core facades,
  dev-only codec. Routine CI (rust/supply-chain/python jobs) verified
  locally via `scripts/verify.sh fast` plus the feature-gated
  clippy/test matrix, doc tests, examples check, both `dist` builds,
  and `verify-cargo-packages.sh --mode all`.
- Dependency proof: `eggserve-primitives` production graph unchanged
  (`bytes`, `futures-util`); `tokio-tungstenite` remains dev-only;
  no H2/H3/QUIC in the default/direct graphs.

## Pre-1.0 migration (see `docs/migration-guide.md`)

Tunnel handlers change arity (`|io, lifecycle|` → `|io|` + captured
lifecycle). `take_tunnel()` is preserved. `is_tunnel()` is always
`false`; `with_tunnel_acceptance`/`take_tunnel_acceptance` are removed.
H2 service-shape convergence is Plan 217 input.
