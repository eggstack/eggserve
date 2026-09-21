# Plan 256 — Post-convergence maintenance and interop fidelity closure

## Campaign baseline

- Planning/audit baseline: `0ee02acd69f1c63d32134f8265283fff04e4630c`
  (remote CI run `35620987177`, green).
- No public Rust/Python API, capability, security-invariant, protocol-tier,
  package-topology, or wire-behavior change is authorized or claimed.

## Implementation landing SHAs (all on `main`)

| Plan | SHA | Content |
|---|---|---|
| 252 | `ee1724b3b56d478399de0856ea82d512144d4307` | Python stub fidelity, strict fixture, runtime shapes, wheel artifacts |
| 253 | `a27c330dbd4f42bf0ef7650bfe5babefd241a689` | Overlap ledger + facade ownership notes (gate rule lands with 255, noted) |
| 254 | `0745635e2091c842871b7780a845cac02ac9f34d` | Async lifecycle matrix + bounded first-pull producer fix |
| 255 | `e359cf1dba69c99ee47d73ab507b459050fd1656` | Import cleanup, checker overlap rule + `--self-test`, Rust profiles |

## What each plan did

### Plan 252 — Python typing/public-surface fidelity

- `lowlevel.pyi`: `AsyncRequest.headers` → `dict[str, str]`,
  `query` → `str` (absent is `""`), `scheme` → `str | None`,
  `remote_addr`/`local_addr`/`effective_addr`/`proxy_source`/
  `proxy_destination` → `str | None` (text form; tuple forms stay on
  `*_address`), `tunnel_request` → `TunnelRequest | None`,
  `AsyncBody.trailers` → `list[tuple[str, str]] | None`,
  `accept(headers)` narrowed to `Sequence[tuple[str, str]] | None`
  (a `Mapping` never survives the passthrough),
  `AsyncResponse.stream(trailers)` → `Sequence[tuple[str, str]] | None`,
  `aiter_chunks` corrected to a plain `def` returning `AsyncIterator`
  (async-generator-function shape, not a coroutine).
- `server.pyi`: supported subclass hooks `log_request`/`log_error`/
  `log_message`, `server_bind`/`server_activate`, and handler attributes
  (`request`, `server`, `close_connection`, `requestline`,
  `error_message_format`, `error_content_type`, `responses`).
- `subprocess.pyi`: `log_format` is `Literal["text", "json", "none"]`;
  private `_parse_bind`/`_config_to_argv` removed from the stub.
- `_native.pyi`: added the runtime-registered but undeclared surface the
  lowlevel stub imports (`PathPolicy`, `StaticPolicy`, `RequestTarget`,
  `SecureRoot`, `Resolved*`, `ResponsePlan`, `ServerRequestError`,
  `validate_*`, `generate_etag`) plus the missing native `Server`
  trusted-proxy kwargs. Runtime (compiled extension + PyO3 registration)
  is authoritative; the stub omitted names the module exports.
- `typing_smoke.py`: strict fixture now exercises property access
  (`assert_type` on every corrected shape), sync/async response
  constructors, `track`, and subclass overrides — not only construction.
  Passes `mypy --strict` under both 1.17.1 (CI pin) and 2.1.0; a
  headers-regression mutation was proven to fail the fixture.
- `test_lowlevel_runtime.py` (`RequestShapeTests`) and
  `test_async_bridge.py` (`AsyncShapeTests`): runtime pins for
  no-query/query, text/tuple address pairs, duplicate header dict vs
  ordered items, and absent/present PROXY metadata.
- `check-wheel-composition.py` now requires all six typed artifacts
  (`py.typed`, `__init__.pyi`, `_native.pyi`, `lowlevel.pyi`,
  `server.pyi`, `subprocess.pyi`).
- Notable: CI's pinned mypy 1.17.1 does not flag missing attributes on
  the native-module import (baseline passed it); the drift was caught by
  a newer checker and fixed for both. The expanded `assert_type`
  fixture does fail 1.17.1 on stub regressions (proven by mutation).

### Plan 253 — connection-overlap classification

- Ledger in `architecture/crate-topology.md`: `transport`,
  `deferred_body`, `lifecycle`, `request` are accepted bounded
  duplication (byte-identical or logic-identical; sharing needs a new
  public Hyper/Tokio transport type — mandatory DEFER); `response` is a
  direct kernel with a core/H3 Alt-Svc post-pass; `pipeline` is H2
  transport glue (kernel merge DEFER); `activity` is a core H2-specific
  delta; `driver` is already split (direct H1 / core H2 + classifier).
- Track C audit: no removable H1-only/dead machinery — every core helper
  has a live H2 caller; H2 modules are already `#[cfg(feature =
  "http2")]`-gated.
- Track E: compatibility facade docs now state H2 ownership explicitly
  (the stale "HTTP/1 connection setup via Hyper" step is corrected).
- Mechanical rule `check_plan253_overlap` (pairs exist, core parallels
  stay crate-private, direct H3-shared surface fixed, H2 gates kept)
  lands in the Plan 255 commit with the checker self-test harness; both
  SHAs are recorded here.
- Guards: `direct_h1_parity` (16), `direct_service_convergence`,
  `cross_protocol_conformance`, canonical conversion suites — all green.

### Plan 254 — async-Python parity

- New `test_async_lifecycle.py` (20 tests, deterministic events, no
  timing sleeps): admission 503/permit return/streaming permit hold/
  error release/no-double-release, timeout-before/after races,
  shutdown-during-handler, chunk ordering/empties/non-bytes truncation/
  exact length/mismatch truncation/sync-iterable convenience, HEAD/204
  suppression, disconnect observation + wait timeout, tunnel
  denial/accept-shutdown ownership, 15× streaming registry closure.
- One reproduced defect, fixed minimally in `lowlevel.py`: producers
  started eagerly, so HEAD/204 advanced the application iterable before
  the Rust consumer dropped it. Producers now wait (bounded by
  `response_write_timeout_secs`) for the first native pull before
  touching application state; un-pulled producers exit quietly with the
  permit released through the existing done-callback path. No permit,
  task-tracking, timeout, or threading-architecture change.
- Truncation expectations match the sync bridge contract
  (`RemoteDisconnected`/`IncompleteRead`, no second response).
- Test-authoring note: blocking the loop thread (`Event.wait`,
  `Thread.join` inside async `main`) deadlocks dispatch; all such waits
  go through `asyncio.to_thread`.
- ASGI fixture untouched and green; docs needed no change (the
  documented suppression semantic is now literally true).

### Plan 255 — residue/module/topology cleanup

- Deleted every broad `allow(unused_imports)` and trimmed to direct
  dependencies (compiler-verified): 10 Python bridge modules, core
  `runtime.rs`/`accept.rs`/`primitives/canonical.rs`, 5 H3 files.
  Genuinely-needed cases kept precisely gated (`#[cfg(test)]`
  `HttpVersion` in the H3 adapter; `#[cfg(feature = "http2")]`
  file-stream re-export in core canonical); dead `Http2Config`/
  `Http3Config` imports deleted.
- NO-CHANGE with rationale: `args.rs` (grammar + co-located tests stay
  one reviewable unit), H3 `adapter.rs` (orchestration locality; moves
  would reopen blocked promotion scope), Python `lib.rs` (shared
  validation helpers stay file-local; splitting scatters invariants),
  static filesystem modules (security-locality, per plan).
- Checker: `check_plan253_overlap` rule, extracted pure predicates
  (`check_forbidden_deps`, `_inventory_diff`), module-level
  `PLAN225_CORE_MODULE_INVENTORY`, and `--self-test` (23 fixture/
  mutation/live checks, all passing; stable entrypoint unchanged).
- Docs: direct-H1 leaf vs compatibility-multiprotocol profiles stated
  once in `README.md` with the authority in
  `docs/public-api-boundary.md` (links, no tutorial duplication).

## NO-CHANGE / DEFER records

- Plan 253: cross-crate helper sharing (new public transport API),
  pipeline-kernel merge, activity shared-core extraction — all DEFER as
  explicit architecture, not defects. Ledger in
  `architecture/crate-topology.md`.
- Plan 255: `args.rs`, H3 `adapter.rs`, Python `lib.rs`, static
  filesystem module splits — NO-CHANGE (see above).
- Plan 254: no defect found in admission/timeout/disconnect/tunnel
  ownership beyond the fixed eager-producer start; no native async API
  or framework semantics added.

## Local qualification (final candidate)

Toolchain: `rustc 1.98.1` / `cargo 1.98.1`, `cargo +1.89.0` for the
pinned check lanes, `Python 3.14.6`, `maturin 1.14.1`, `mypy 1.17.1`
(CI pin; 2.1.0 additionally), `unittest` suite runner.

- `verify-conformance-matrix.py`: 51 matrix + 55 app-server (47
  routine) + 17 H3 entries valid.
- `check-crate-topology.py`: gate green; `--self-test` 23/23.
- `check-python-release-metadata.py`: pass (0.2.0).
- `cargo fmt --all -- --check`: clean.
- `cargo +1.89 check --workspace --all-targets` (default,
  `http2,tls`, `http3,tls`): clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`:
  clean; plus `-p eggserve-core` (`http2,tls` / `http3,tls`) and
  `-p eggserve-bin` (`http2,tls` / `tls` / `http3,tls`): clean.
- `cargo test --workspace`: 1957 passed, 4 ignored.
- `cargo test -p eggserve-core --features http2,tls`: 1136 passed.
- `cargo test -p eggserve-core --features http3,tls`: 1137 passed.
- `cargo test -p eggserve-bin` (`http2,tls` / `tls` / `http3,tls`):
  141 each.
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml
  --locked`: clean.
- `check-supply-chain.sh`: advisories/bans/licenses/sources ok.
- `verify-cargo-packages.sh --mode all`: layered crates pass.
- `test-python-wheel.sh`: full installed-wheel run green — 832 tests
  OK (includes 20 new lifecycle + 8 new shape tests), strict mypy
  fixture green, strengthened composition gate green, smoke green.

## Remote CI provenance

- Candidate SHA: `4c145421c851fffa5e1f6762a7ef742c5db1e5d8`
- Run ID: `35653232800`
- URL: `https://github.com/eggstack/eggserve/actions/runs/35653232800`
- Created/completed: `2026-09-21T20:47:10Z` / `2026-09-21T21:02:32Z`
- Conclusions: `rust` success, `supply-chain` success, `python` success.

## Acceptance

All Plan 256 acceptance criteria hold on the candidate above. Plans
251–256 are closed; unrelated future defects get their own corrective
plans. A later metadata-only commit records these run IDs in this file
and flips the roadmap line to complete; that commit is documentation
only and is recorded separately below.

- Metadata-only documentation SHA: recorded in git history (commit
  `docs: record Plan 256 remote CI provenance (metadata-only)`; see
  `git log --oneline`). Per the closure rule it is not claimed as an
  independently qualified candidate.
