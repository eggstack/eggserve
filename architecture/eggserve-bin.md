# eggserve-bin — Deep Dive

The static-only CLI binary crate (`eggserve-bin 0.2.1`). `main.rs` is a shim
over `lib.rs` (`run`/`run_cli`); `args.rs` owns the manual grammar (no clap);
`shutdown.rs` owns signal handling; `tls.rs` re-exports the neutral TLS
substrate. Neutral policy/observability/static paths name the leaf crates
directly (Plan 221); extended TLS/H2/H3 orchestration goes through the closed
compatibility facade (Plan 225). Published alongside the `0.3.0` leaves via
Plan 286 (lockfile-only `0.2.1` selection; no behavior change).

## Module map

| Module | Purpose |
|--------|---------|
| `main.rs` | Thin `fn main()` → `eggserve_bin::run()` (`src/main.rs:1-3`) |
| `lib.rs` | `run()` (parse → exit code) and `run_cli(argv) -> i32` (same syntax, no `exit`); current-thread Tokio runtime; dual `cfg(tls)` orchestration paths |
| `args.rs` | Manual `[OPTIONS] [PORT] [DIRECTORY]` grammar, `require_value` flag-guard, `--header=NAME=VALUE` expansion, `validate_static_metadata` gate |
| `shutdown.rs` | Ctrl+C / SIGTERM / SIGHUP → `broadcast::Sender<()>` graceful-stop relay |
| `tls.rs` | Re-export of `eggnet_tls::*` (Plan 221; single-identity PEM loading lives in the neutral substrate) |

## Entrypoints

```rust
pub fn run()                          // `std::env::args` → `run_cli` → `process::exit`
pub fn run_cli(argv: Vec<String>) -> i32  // extension-backed CLI plumbing
```

`run_cli` parses `argv`, builds `ServeConfig` + `Limits`, inits the global
`Logger` once via `try_init`, loads optional TLS identity, projects
`try_from_serve_config`, builds `Server`, holds a broadcast receiver across
`start()`, logs `ListenerReady`, waits for the first signal (closed channel =
fail-safe shutdown), then `shutdown()` + `timeout(grace, handle.wait())`.
Dirty stops (`Err` or timeout) return `1`. It exists for the wheel's native
`_run_cli` (`eggserve-python/src/lib.rs`) / `python -m eggserve`; it is not a
general Rust embedding API — Rust applications use `eggserve-core` facades or
the direct leaves (see `crates/eggserve-bin/src/lib.rs:27-34`).

## Args grammar (`args.rs`, no clap)

Canonical grammar is `[OPTIONS] [PORT] [DIRECTORY]` (normative flags in
`docs/cli.md`; timeout semantics in `docs/timeout-reference.md`):

* `--directory DIR` occupies DIRECTORY; `.` default. `--bind HOST[:PORT]`
  (host-only leaves PORT free), `--port PORT`, `--addr HOST:PORT` (cannot
  combine with `--bind`); `:PORT` means `0.0.0.0:PORT` (still needs
  `--public`). Hostnames resolve once; wildcard results still need `--public`.
* Two logical slots: PORT then DIRECTORY. Explicit port sources occupy PORT;
  the next positional after occupied PORT is DIRECTORY verbatim (even numeric).
  `--directory` occupying DIRECTORY leaves a later numeric positional free for
  PORT. Excess positionals rejected; padded/signed/out-of-range numerics
  rejected as ports (verbatim only once PORT is occupied); `--` ends options.
* Every non-repeatable flag errors on repeat; value flags never swallow a
  following flag (`--bind requires an argument (found flag ...)`); bare `-`
  is a literal value.
* Policy: `--directory-listing`, `--follow-symlinks`, `--allow-dotfiles`
  (all default deny/off). Metadata: `--content-type`, repeatable
  `-H/--header NAME VALUE` (also `--header=NAME=VALUE`); ordered, validated by
  `validate_static_metadata`, final-200-only, runtime/hop-by-hop rejected.
* Limits/timeouts/parser ceilings: `--max-connections`, `--max-file-streams`,
  `--max-in-flight-requests`, `--max-requests-per-connection` (`0` =
  unlimited), `--header-timeout`, `--connection-total-timeout` (`0` opts out
  of only the hard total lifetime per Plans 270–271), `--handler-timeout`,
  `--body-read-timeout`, `--keep-alive-idle-timeout`,
  `--response-write-timeout`, `--max-buf-size`, `--max-headers`,
  `--max-header-bytes`, `--max-request-target-bytes`.
* Output: `--log-format text|json|none`, `--quiet` (warn/error filter),
  `-h/--help`, `-V/--version`.
* TLS (feature `tls`): `--tls-cert PATH` (+ optional `--tls-key PATH`;
  omitted key falls back to cert path for combined PEM). H3 (feature `http3`):
  `--http3` requires TLS identity, enables the same-port QUIC endpoint and
  `Alt-Svc` advertisement.

## Accept loop architecture

The accept loop lives in `eggserve-core::server` (`accept_loop_multi`,
Plan 201). Both TLS and non-TLS paths use `Server::builder()` →
`Server::start()`. Per Plan 249, compatibility `Auto` classification resolves
before any Hyper service exists: every H1 path delegates the replayable
stream to the direct `eggserve-server` H1 driver
(`connection::serve_http1_connection`), while core executes H2 only (see
`../release/plan-250-h1-authority-lifetime-corrective-closure.md`).
When `RuntimeConfig.tls_config` or `tls_reload_handle` is set (Plan 203
reload handle wins atomically), the accept loop performs a per-connection TLS
handshake via `tokio_rustls::TlsAcceptor` (order
`TCP → PROXY → TLS deadline → ALPN → HTTP`) before dispatching to the HTTP
connection handler. CLI remains single-identity; Rust `TlsServerConfig`
provides SNI/mTLS/reload.

```
┌─────────────────────────────────────────────┐
│ accept_loop_generic()                       │
│  • TCP accept with connection semaphore     │
│  • Lifecycle state machine                  │
│  • Spawn Tokio task per connection          │
│  • (TLS): per-connection TLS handshake      │
│  • On shutdown signal: drain and stop       │
└─────────────────┬───────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────┐
│ per-connection handler                      │
│  • Read headers with header_read_timeout    │
│  • Call service with StaticService           │
│  • Write response with connection_total_timeout│
│  • Drop semaphore permit on completion      │
└─────────────────────────────────────────────┘
```

When the semaphore is exhausted, new connections are dropped immediately
(connection limit enforcement).

## TLS loading

Behind `tls` (`eggserve-core/tls`). `tls.rs` is `pub use eggnet_tls::*`;
loading/validation (bounded PEM, PKCS#1/8/SEC1, exactly-one-key,
`keys_match`, no key-material logging) lives once in `eggnet-tls`. CLI stays
single-identity; SNI/mTLS/reload is Rust-first (`TlsServerConfig`). The
`tls`-built binary still serves plaintext when no cert is given (logged
`scheme` is `http` vs `https`). `--http3` additionally calls
`http3_identity(cert, key)` on the builder; H3 keeps its separate TLS
1.3/QUIC identity (TCP reload does not rotate H3).

## Shutdown (`shutdown.rs` + `lib.rs`)

`broadcast::channel(1)` receiver is created before the signal task and held
through `start()` so a signal during startup is buffered, not lost. Handled:
Ctrl+C (all), SIGTERM/SIGHUP (Unix; SIGHUP = graceful stop, not terminate).
Only the first signal acts; further signals during drain are consumed without
escalation. After the signal: log `ShutdownRequested` with the grace period,
`handle.shutdown()`, then `timeout(grace, handle.wait())` → log
`ShutdownComplete` (`Clean` → `0`; `Err`/timeout → warn/error + return `1`).

## Plan 221 leaf naming (closed facade)

Binary neutral paths name leaves directly: `eggserve-primitives` (policy),
`eggserve-server` (ops/logging, shared limits), `eggserve-static` (direct-H1
unit tests: leaf `Server` + leaf `StaticService`, no core import),
`eggnet-tls` (loading). Compatibility-owned orchestration (the closed Plan
225 set): `ServeConfig`/`try_from_serve_config`, full `Server` (TLS/H2/H3),
full `StaticService` (extra headers/error policy), `Limits`/static-metadata
validation with static budgets. Do not reintroduce core indirection on neutral
paths; core removal needs a separate migration plan.

## Dependencies

| Dependency | Purpose |
|------------|---------|
| `eggserve-core` | Closed extended orchestration only (see above) |
| `eggserve-primitives` / `eggserve-server` / `eggserve-static` / `eggnet-tls` | Direct neutral authorities (Plan 221) |
| `eggserve-h3` (optional, `http3`) | Same-port QUIC endpoint assembly via core orchestration |
| `tokio` | Current-thread runtime + signal/broadcast/time |

Minimal build pulls no H3/Tower/Python-only deps. `cargo test -p eggserve-bin`
covers grammar + direct-H1 proofs; `verify.sh full` adds TLS/H2/H3-gated
suites.

## See also

* `docs/cli.md` — normative flag/grammar contract
* `docs/timeout-reference.md` — timeout semantics
* [eggserve-core.md](eggserve-core.md) — closed compatibility facade
* [crate-topology.md](crate-topology.md) — Plan 221/225/249/276 gates
* [security-model.md](security-model.md) — safe defaults behind the flags
* [overview.md](overview.md) — crate map and request lifecycle
* `release/plan-286-embedding-contract-publication-closure.md` — `0.2.1` publication evidence
