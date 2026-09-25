# Architecture Overview

EggServe is a hardened, HTTP-correct static file server and reusable Rust
HTTP/static-serving library, with a Python `http.server`-shaped facade. Static
serving is the primary product; a separate downstream project may use the
qualified HTTP-only Rust substrate. The CLI is static-only; the Python facade
adds bounded synchronous custom handlers; `eggserve.lowlevel` exposes a
handler-only runtime/service substrate (plus the experimental H1-only async
substrate) for downstream bounded application servers; `eggserve-core::server`
exposes an experimental low-level Rust service boundary. EggServe is not an
application framework, ASGI/WSGI runtime, CGI executor, FastCGI gateway,
proxy, or general-purpose `socketserver` replacement.

**This document is the entry point for understanding the codebase.** Each
section below gives a discrete 2–4 sentence bird's-eye overview of one
module, tool family, or capability, then links to the deep-dive document that
owns the detail. Use the [Deep Dive Index](#deep-dive-index) to jump to any
subsystem for a focused review.

Plan context: the authority split (Plans 211–225), maintainability
convergence (Plans 243–250), and post-convergence maintenance (Plans 251–258)
are closed; Plans 259–260 are docs-only refreshes (overview index + deep-dive
refresh, no behavior/tier change). H1 + canonical `primitives` are supported; `server`/H2/H3/
tunnel/trailer/adapter/listener/proxy/TLS-identity/async-Python remain
experimental. `plans/` + `ROADMAP.md` are change-trace records, not normative
API docs — normative user contracts live in `docs/`.

Plans 270–273 add the direct H1 control/completion split and opt-in disabled
total connection lifetime, qualify the combined leaf-crate embedding contract,
publish the additive 0.2.1 Rust patch, and prove it from a clean registry-only
consumer. The direct-server downstream embedding contract is registry-qualified;
see [the closure evidence](../release/plan-272-downstream-embedding-qualification-closure.md).

Plans 274–275 repair the optional HTTP/Tower request-body ownership boundary,
qualify Axum 0.8 with direct `eggserve-server` composition, and publish the
core-only 0.2.2 patch (see
[the closure evidence](../release/plan-275-http-tower-adapter-patch-publication-closure.md)).
Plans 276–277 extract adapter ownership to `eggserve-server` (core forwards
`http-interop`/`tower` as compatibility re-exports; direct H1 + Tower graph
stays free of `eggserve-static`/PHF); the standalone 0.2.3 was folded into
Plan 286, never published alone. Plans 278–279 add the opt-in
forward-proxy absolute-form seam (`Http1RequestTargetMode::OriginOnly`
default vs `OriginOrAbsolute`; static stays origin-only and rejects
absolute-form; see
[the seam closures](../release/plan-278-forward-proxy-seam-implementation-closure.md)).
Plans 280–286 are the direct H1 embedding policy-ownership program:
external deadline/ceiling ownership (`PolicyOwner`), external admission
ownership (`AdmissionOwnership`), narrow `H1ConnectionPolicy` projection,
presentation-only typed rejection (`RuntimeRejectionKind`/`RuntimeRejection`),
TunnelIo direct-transport KEEP (Plan 284), embedding-contract qualification
(Plan 285), and publication of `primitives 0.2.1` + `server`/`static`/`h3`/
`core 0.3.0` + `bin 0.2.1` with registry-only proof (Plan 286; see
[the closure evidence](../release/plan-286-embedding-contract-publication-closure.md)).
Current versions: `eggserve-server`/`eggserve-static`/`eggserve-h3`/
`eggserve-core` at `0.3.0`, `eggserve-bin` at `0.2.1`, `eggserve-python`
wheel at `0.2.3`.

## What eggserve Is

- **A hardened static file server** — serves files from a directory with
  confinement and HTTP-correctness guarantees.
- **A CLI tool** — `eggserve` binary with `--directory`, `--bind`, port,
  TLS, and policy flags (static-only).
- **A Python package** — `eggserve` wheel with `python -m eggserve` and an
  `http.server`-compatible API, plus a `lowlevel` handler-only substrate.
- **A reusable Rust library** — leaf crates (`eggserve-primitives`,
  `eggserve-server`, `eggserve-static`, `eggnet-tls`, `eggserve-h3`) provide
  the direct implementation authorities; `eggserve-core` is the
  compatibility/composition umbrella over them.

## What eggserve Is Not

- Not an ASGI/WSGI server, CGI executor, FastCGI gateway, or web framework.
- Not a reverse proxy, ACME client, or plugin host.
- Not an HTTP client or outbound CONNECT/proxy client (Plan 223: the shared
  outbound H1 CONNECT wire primitive lives outside eggserve for
  eggfetch/eggress; eggserve owns only inbound tunnel acceptance).
- Not a file upload handler, auth system, or template engine.
- Not a WebSocket framing server (generic tunnel handoff per Plan 199;
  framing stays downstream).

Plan 167 closed as no-go: no in-tree CGI/FastCGI adapters. Downstream
gateways implement the canonical `Service` trait and return canonical
`Response` values; see [runtime.md](runtime.md) and
[../docs/extension-contract.md](../docs/extension-contract.md). Plan 199
implements generic tunnel/upgrade/Extended CONNECT (one-shot
transport-backed capabilities; `accept` returns a handshake `Response`
plus bounded `TunnelIo`; denial stays ordinary HTTP). Plan 216 moves tunnel
authority to the direct crates (neutral vocabulary in
`eggserve-primitives`, execution in `eggserve-server`). Plan 223 keeps that
authority inbound-only. See [../docs/non-goals.md](../docs/non-goals.md) and
[../docs/downstream-app-server.md](../docs/downstream-app-server.md).

The user-facing Python compatibility matrix is maintained in
[`docs/python-http-server-compatibility.md`](../docs/python-http-server-compatibility.md).

## Core Invariants

1. **Safe defaults are not defaults if they can be overridden silently.**
   Loopback bind, no symlinks, no dotfiles, no directory listing unless the
   user explicitly opts in.
2. **No serving outside the configured root.** Traversal and symlink escape
   denied at library level (Unix safe defaults: descriptor-relative
   `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`).
3. **No broad dependencies.** Every dependency has an explicit purpose
   (see `../docs/dependency-policy.md`).
4. **Plan-driven development.** Every change traces to a plan in `plans/`.

---

## Workspace Layout

```
eggserve/
├── Cargo.toml                  # workspace root (resolver = "2", edition 2021)
├── crates/
│   ├── eggnet-tls/             # neutral rustls identity/trust/reload substrate
│   ├── eggserve-primitives/    # canonical transport-neutral model
│   ├── eggserve-server/        # single mature H1 connection runtime + Service
│   ├── eggserve-static/        # SOLE static/path/filesystem authority
│   ├── eggserve-h3/            # experimental H3/QUIC adapter (sole QUIC owner)
│   ├── eggserve-core/          # compatibility/composition umbrella (facades)
│   ├── eggserve-bin/           # static-only CLI binary
│   └── eggserve-python/        # Python wheel (maturin + PyO3, excluded from workspace)
├── architecture/               # this directory — one deep dive per component
├── docs/                       # normative reference docs (user contracts)
├── plans/                      # historical design/implementation records
├── release/                    # per-plan qualification/closure records
├── conformance/                # shared Rust/Python conformance corpora
├── fuzz/                       # fuzz targets + seed corpora (11 targets)
├── benchmarks/                 # benchmark evidence (profiles + result files)
├── tests/                      # repo-level interop/soak shells
├── scripts/                    # verification hierarchy + package/release checks
└── examples/                   # canonical CLI/Python/Rust demos (see README index)
```

---

## Crate Architecture (discrete module overviews)

Strict downward-only dependency hierarchy. `eggserve-static` owns all
static/path/filesystem decisions; `eggserve-server` owns the H1 runtime;
QUIC dependencies live only behind `http3`; `eggserve_bin::run_cli` is
plumbing for the wheel's extension CLI, not a general embedding API. Checked
by `scripts/check-crate-topology.py`; see
[crate-topology.md](crate-topology.md).

```
eggnet-tls              (leaf: neutral rustls substrate, no internal deps)

eggserve-primitives     (leaf: canonical values, transport-neutral deps only)
        │
        ▼
eggserve-server         (H1 runtime + Service; depends on primitives only)
        │
        ▼
eggserve-static         (static/path/FS authority; consumes primitives+server)

eggserve-h3             (experimental QUIC adapter; consumes primitives/server/eggnet-tls)

eggserve-core           (compatibility umbrella; facades over the leaves + H2/TLS/proxy glue)
        │
        ├── eggserve-bin      (static-only CLI; neutral paths name leaves directly, Plan 221)
        └── eggserve-python   (wheel; neutral bridge names leaves directly, Plan 221)
```

### Leaf crates (direct authorities)

| Crate | Bird's-eye overview | Deep dive |
|-------|---------------------|-----------|
| `eggnet-tls` | Neutral rustls identity/trust/client-auth/reload substrate: bounded PEM parsing, key/cert pairing, SNI (exact + single-level wildcard), WebPKI mTLS modes, trust/CRL bounds, atomic reload snapshots, neutral ALPN hook. No HTTP, Tokio, QUIC, proxy, or filesystem-watcher dependency; production graph is `rustls` + `rustls-pki-types` only. | [eggnet-tls.md](eggnet-tls.md), [tls.md](tls.md) |
| `eggserve-primitives` | Canonical transport-neutral request/response/body/lifecycle/policy model: `Method`, `HttpVersion`, `HeaderBlock`, `RequestTarget`, `RequestHead`, `Request`/`RequestBody`/`RequestContext`/`RequestLifecycle`, `StatusCode`, `ResponseHead`/`ResponseBody`/`Response`, `BodyLength`, trailers/interim, proxy metadata, tunnel vocabulary, `StaticPolicy`, `Limits`. Small transport-neutral deps only (`bytes`, `futures-util`). | [eggserve-primitives.md](eggserve-primitives.md), [primitives-api.md](primitives-api.md) |
| `eggserve-server` | Single mature H1 connection runtime and transport boundary (`0.3.0`): strict-H1 driver over any `AsyncRead + AsyncWrite` stream, single `Service` contract (+ additive `call_with_tunnel`), `RuntimeConfig`/`RuntimeState` admission pool, per-connection shutdown, listener TCP `Server`, tunnel execution (incl. Plan 284 direct opaque `TunnelIo`), `OpsContext` authority, response policy, shared limit kernel. Plans 270/280–283 add supervisory `ServerControl`/`ServerCompletion`, external policy/admission ownership with narrow `H1ConnectionPolicy` projection, typed rejection presentation, and opt-in absolute-form dispatch (Plan 278). Plans 276–277 add opt-in `http-interop` and Tower adapters for direct H1 composition. Depends on primitives only; never on static/core. `http2`/`tls` are inert compatibility feature names (H1-only crate). | [eggserve-server.md](eggserve-server.md), [runtime.md](runtime.md) |
| `eggserve-static` | SOLE static/path/filesystem/MIME/planning authority: `ConfinedPath` parsing, `SecureRoot`/`PinnedRoot` confinement, descriptor-relative (Unix) / handle-relative (Windows) resolution, capability bridge, MIME selection, conditional/range/ETag planner, `StaticService` request planning + rendering. Core keeps facades only. Plan 224 NO-GO: no capability-filesystem crate. | [eggserve-static.md](eggserve-static.md), [path-confinement.md](path-confinement.md), [filesystem-confinement.md](filesystem-confinement.md), [response-planning.md](response-planning.md) |
| `eggserve-h3` | Experimental H3/QUIC transport adapter and sole QUIC dependency owner (`h3`/`h3-quinn`/`quinn` pinned set). Downward-only on primitives/server/`eggnet-tls`. Owns endpoint lifecycle, bounded QUIC/H3 policy, canonical request/response/tunnel adaptation, Alt-Svc. Blocked upstream (`hyperium/h3#338`, `#262` remainder); stays experimental. | [eggserve-h3.md](eggserve-h3.md), [http3.md](http3.md) |

### Composition crates (frontends over the leaves)

| Crate | Bird's-eye overview | Deep dive |
|-------|---------------------|-----------|
| `eggserve-core` | Compatibility/composition umbrella (`0.3.0`; 0.2.1 via Plan 272, core-only 0.2.2 Tower repair via Plans 274–275): facades over the direct authorities (no second implementation) plus explicit transport glue (H2 wire mechanics, TLS/proxy/listener composition, extended `Server`/`ServerBuilder` orchestration, `ServeConfig`/`try_from_serve_config`, `StaticService` wrapper, thin H3 facade). Plan 249: compatibility `Auto` classifies before any Hyper service exists; core executes H2 only. Forwards server-owned `http-interop`/`tower` as compatibility re-exports; projects `OriginOnly` absolute-form default. Plan 225 facade closure; `0.3.0` breaking embedding contract via Plans 285–286. | [eggserve-core.md](eggserve-core.md), [runtime.md](runtime.md), [configuration.md](configuration.md) |
| `eggserve-bin` | Static-only CLI binary (`0.2.1`; lockfile-only selection via Plan 286, no behavior change). `main.rs` is a shim; real logic lives in `lib.rs` (`run`, `run_cli`) + `args.rs` (manual `[OPTIONS] [PORT] [DIRECTORY]` grammar, no clap) + `shutdown.rs` (signal handling) + `tls.rs` (neutral substrate re-export). Neutral policy/ops/limits/static paths name the leaf crates directly (Plan 221); extended orchestration goes through the core facade. | [eggserve-bin.md](eggserve-bin.md) |
| `eggserve-python` | Workspace-excluded maturin/PyO3 (`0.29.2`, `abi3-py311`, CPython 3.11–3.15 build-once/test-many) wheel (`0.2.3`; Python distribution, Rust side tracks `0.3.0` leaves). `eggserve.server` six-class `http.server`-shaped facade (stock `SimpleHTTPRequestHandler` fast path only for the exact bare class), `eggserve.lowlevel` handler-only substrate (`RuntimeConfig`, `Server`, `Response.stream` over a 16-chunk bridge, `StaticResponder` composition; experimental H1-only `AsyncServer`), `eggserve.subprocess` lifecycle helpers. Bridge in `src/server/` submodules; async stays Python-side. H1-only facade (no new protocol surfaced to Python). | [eggserve-python.md](eggserve-python.md) |

### Feature flags

| Feature | Crates | Purpose |
|---------|--------|---------|
| `tls` | `eggnet-tls`, `eggserve-core`, `eggserve-bin`, `eggserve-python` | Neutral rustls identity/trust policy + EggServe async TLS transport |
| `http2` | `eggserve-core`, `eggserve-bin` | Experimental bounded HTTP/2 runtime; Python remains H1-only |
| `http3` | `eggserve-h3`, `eggserve-core`, `eggserve-bin` | Experimental bounded HTTP/3/QUIC runtime; separate TLS 1.3/`h3` identity, same-port UDP; Python remains H1-only |
| `python-bindings-internal` | `eggserve-core` → `eggserve-static` | Internal capability bridge (`ResolvedFile` extraction) for the wheel |
| `windows-adversarial-qualification` | `eggserve-core` | Windows adversarial qualification gates |

---

## Capability Map (discrete capability overviews)

Each capability is 2–3 sentences. Follow the link for the review deep dive.

| Capability | Bird's-eye overview | Deep dive |
|------------|---------------------|-----------|
| Static serving | Primary product. `StaticService` validates the method, parses the target to a `ConfinedPath`, resolves via `SecureRoot` to a `ResolvedResource`, then plans the response (conditional/range/ETag, HEAD parity, directory policy) and streams the file or renders listing/error. Planning authority lives once in `eggserve-static`. | [eggserve-static.md](eggserve-static.md), [response-planning.md](response-planning.md), [runtime.md](runtime.md) |
| Path confinement | 6-stage pipeline (`RequestTarget::parse` → decode → normalize → split → validate → `ConfinedPath`), 17 `PathRejection` variants, dual `DotfilePolicy` types that must agree. Origin-form classification is the sole target-form classifier. | [path-confinement.md](path-confinement.md) |
| Filesystem confinement | Post-validation resolution through `PinnedRoot`/`RootGuard`: Unix descriptor-relative `statat`+`openat`, Windows handle-relative resolution with reparse-point denial, TOCTOU-safe open semantics. Confinement guarantee ends if the caller extracts the raw handle. | [filesystem-confinement.md](filesystem-confinement.md) |
| Policy system | Layered safe-defaults policy: `StaticPolicy.symlinks` (not `follow_symlinks`), dotfile/listing controls, `StaticMetadataPolicy`, `ErrorRepresentationPolicy`, `ResponsePolicy`/`DatePolicy`/denylist. Every unsafe behavior needs an explicit opt-in flag. | [policy-system.md](policy-system.md), [configuration.md](configuration.md) |
| H1 runtime + service contract | One `Service::call(Request) → Response` contract drives direct H1 and (via the same canonical types) compatibility H2. Single H1 authority: compatibility `Auto` classifies first, every H1 path delegates to `eggserve-server`; core executes H2 only (Plan 249). Per-connection shutdown is structured under the connection task. Supervisors use `ServerHandle::into_parts()` + cloneable `ServerControl` with cancellation-safe `ServerCompletion::wait()` (Plan 270); `Duration::ZERO` opts out of only the total lifetime. Embedding owners can take external deadline/ceiling/admission ownership with a narrow `H1ConnectionPolicy` projection (Plans 280–282) plus a presentation-only typed-rejection hook (Plan 283). Opt-in absolute-form dispatch (`OriginOrAbsolute`; static still rejects, Plan 278). Response framing belongs to the runtime only. | [runtime.md](runtime.md), [eggserve-server.md](eggserve-server.md) |
| Request bodies + lifecycle | `RequestBody` is one-shot (read-all or streaming, once); default policy `Reject`. Stream bodies share an Active→Complete/Abandoned/Failed lifecycle with disconnect/shutdown observers; `RequestContext` is the single attachment point (connection + lifecycle + bounded interim sender). Trailers are terminal-only with separate bounds. | [runtime.md](runtime.md), [primitives-api.md](primitives-api.md), [error-taxonomy.md](error-taxonomy.md) |
| Tunnel / upgrade | Generic inbound-only tunnel acceptance: neutral intent vocabulary in `primitives::tunnel`, execution in `server::tunnel`, one-shot transport-backed `TunnelIo`, handshake `Response` (`101` H1 / `200` otherwise), denial as ordinary HTTP. Plan 284 KEEP: opaque direct transport alongside the duplex `Pair` fixture (read-ahead preserved, tracked drain). No outbound CONNECT stack; WebSocket framing stays downstream. | [runtime.md](runtime.md) |
| TLS | Split ownership: `eggnet-tls` owns neutral identity/trust/mTLS/reload policy; consumers own TLS transport. Production SNI/mTLS/reload, explicit bounds (identities, roots, CRLs, ALPN), `max_early_data_size=0`, atomic rotation for new handshakes. H3 keeps a separate TLS 1.3/QUIC identity. CLI/Python stay single-identity; advanced TLS is Rust-first. | [tls.md](tls.md), [eggnet-tls.md](eggnet-tls.md) |
| H2 (experimental) | Opt-in Hyper-backed H1/H2 selection via ALPN or bounded prior-knowledge classifier: stream-scoped reject, bounded transport policy, no-progress stall fallback, GOAWAY handling, Extended CONNECT for validated tunnels. Deterministic + interop qualified; browser/trailer-determinism/reset-hook gaps keep it experimental. | [http2.md](http2.md) |
| H3 (experimental) | Opt-in QUIC adapter behind `http3`: same-port UDP lifecycle, shared connection/service/file budgets, 0-RTT disabled, canonical adapters, per-stream producer/send-progress stall reset. Deterministic corrections landed (Plans 190/192/194/195); missing independent-client/adversarial/platform evidence + upstream blockers keep it experimental. | [http3.md](http3.md), [eggserve-h3.md](eggserve-h3.md) |
| Python facade | Six-class `eggserve.server` facade (incl. rustls `HTTPSServer` with H1-only ALPN), exact-bare-class fast path vs Python callback path, `default_content_type` + ordered safe `extra_response_headers` (final 200s only), `protocol_version` stays HTTP/1.1. Intentional incompatibilities are enumerated, not accidental. | [eggserve-python.md](eggserve-python.md) |
| Python `lowlevel` + async | Handler-only `Server(config, handler)` on the same native runtime (no static root, no second accept loop), frozen `RuntimeConfig` safe subset, 16-chunk `Response.stream` (sync iterables only), caller-owned `StaticResponder`, `effective_*` proxy getters. Experimental H1-only `AsyncServer` with manual asyncio bridge and ASGI test fixture only. | [eggserve-python.md](eggserve-python.md) |
| Listeners + proxy metadata | One `accept_loop_multi` drives TCP + Unix (prebound TCP/Unix, systemd index/name, same-port H3 UDP validation). Stable `tcp-0`/`unix-0` endpoint IDs; Unix is plaintext without unlink. Opt-in trusted-proxy (explicit peers/CIDRs, PROXY v1/v2, `Forwarded`/`X-Forwarded-*` single-hop rightmost-wins, fail-closed conflicts). Still no reverse proxying. | [runtime.md](runtime.md), [configuration.md](configuration.md) |
| Configuration | Split ownership: `RuntimeConfig`/`Limits` own transport/admission (timeouts, semaphores, parser bounds), `ServeConfig` owns filesystem composition (root, static policy, file streams). Validated once against `Semaphore::MAX_PERMITS`; per-profile defaults in `docs/deployment.md`, full semantics in `docs/timeout-reference.md`. | [configuration.md](configuration.md) |
| Errors | Five layers, never conflated: `PathRejection` (path validation) / `RequestValidationError` (HTTP-level, Python-facing) / `ServerError` (lifecycle, non-exhaustive, incl. `Terminal` surfaced via typed completion) / `ServiceError` (struct over a private kind; inspect via `is_panic`/`is_timeout`) / `RequestBodyError` (body consumption, non-exhaustive). Plus the presentation-only `RuntimeRejectionKind`/`RuntimeRejection` hook (Plan 283; status/framing/privacy stay runtime-owned). Cancellation/outcome enums are non-exhaustive (match with wildcard). Never synthesize a second HTTP error after final commitment. | [error-taxonomy.md](error-taxonomy.md) |
| Observability | Event-model logging (`Event`/`EventKind`/`Severity`, `LogSink`, `OpsCounters`, per-runtime `OpsContext`). Runtime code emits via its explicit context (`ops.emit(...)`); `Logger::global()` is CLI/frontend-init only (`OnceLock`: `try_init`, never double-`init`). Sinks contain child panics via `dropped_log_events`; library code never `println!`/`eprintln!`. | [structured-logging.md](structured-logging.md) |
| Security model | Central invariant + 7 defensive layers (path, policy, filesystem, input validation, resource limits, response normalization, sanitized logging) with an explicit attacker/trust-boundary statement and per-platform confinement account (descriptor-relative Unix, handle-relative Windows). | [security-model.md](security-model.md) |

---

## Tool Map (discrete tool overviews)

### Verification scripts (`scripts/`)

`verify.sh` is the tier dispatcher; the Python gate scripts run before any
build. Routine CI's `rust` job additionally runs the wheel-target authority
step (`wheel-matrix.py validate`/`self-test`, `check-release-wheel-set.py
--self-test`, `check-release-workflow.py` + `--self-test`) and the full
MSRV (`cargo +1.89`) matrix — those gates are CI-only, not part of
`verify.sh fast`. See [testing-and-conformance.md](testing-and-conformance.md)
for how each layer fits `fast`/`full`/`deep`.

| Script | What it does |
|--------|--------------|
| `verify.sh fast` | Routine dev check: conformance-matrix + topology + release-metadata gates, MSRV (`cargo +1.89`) http-interop/Tower checks, Tower clippy+test lanes, `fmt`, workspace `clippy`/`test`, H2/H3-gated `clippy`/`test`, excluded-crate `cargo check` |
| `verify.sh full` | `fast` + TLS (`eggserve-bin`) tests + `cargo check/build -p eggserve-core --examples` + `test-examples.sh` + Python wheel build/test + package dry-run (needs Python 3.14 + maturin; `PYTHON=` overrides, else dies) |
| `verify.sh deep` | `full` + `corpus_replay`, `stateful_fuzz_replay`, `fault_injection`, `filesystem_race_qualification`, `tls_abuse --features tls`, proxy interop when `caddy`+`nginx` are present (skips otherwise unless `EGGSERVE_REQUIRE_PROXY=1`) |
| `verify-conformance-matrix.py` | Schema + domain validator for `conformance/*.toml`: 51-entry H1 static `[[matrix]]`, 55-scenario app-server inventory, 17-scenario H3 inventory (runs first in CI) |
| `check-crate-topology.py` | Enforces Plans 211–253 ownership/dependency/facade/orphan-source/feature rules (+254–258 notes): primitives leaf, server never pulls static, static owns path/FS, H3 owns QUIC downward-only, no capability-filesystem crate (Plan 224 NO-GO), core is classified facades only |
| `check-python-release-metadata.py` | Cheap version + `[profile.dist]` + entry-point + abi3/classifier sync check (workspace `Cargo.toml` ↔ python-crate `Cargo.toml` ↔ `pyproject.toml` ↔ `__init__.py`) |
| `test-python-wheel.sh` | Authoritative wheel harness (routine Python CI + `verify.sh full`): metadata preflight → maturin build (or `WHEEL_PATH` reuse for the Plan 264 build-once/test-many ABI proof) → fresh venv → smoke + pytest (`MODE=full` suite or `MODE=abi-smoke` lane; default interpreter `python3.14`, `PYTHON=` overrides) |
| `test-examples.sh` | Stdlib-only Python harness: smoke-tests the built canonical **Rust** examples on loopback port `0` (real HTTP request + clean shutdown each) |
| `verify-cargo-packages.sh --mode all` | Release-prep crates.io package dry-run: stages a temporary publish-shaped workspace in dependency order, validates exact `.crate` contents via a file-backed local registry (nothing uploaded) |
| `install-cargo-tools.sh` / `check-supply-chain.sh` | Pinned `cargo-audit`/`cargo-deny` install, then advisory audit + policy check over **both** lockfiles (root `Cargo.lock` + `crates/eggserve-python/Cargo.lock`) |
| `qualify-http2.sh` / `qualify-http3.sh` | Manual wire-qualification harnesses, intentionally outside routine CI. Missing evidence is SKIP, never PASS (fail-closed `EGGSERVE_REQUIRE_*` gates for two-client/browser/platform/adversarial/impairment evidence) |
| `check-wheel-composition.py` | Wheel-content guard: extension-backed CLI only — fails on bundled executables, requires `py.typed` + type stubs |
| `check-release-wheel-set.py` | Matrix-driven release-set validator against `release/wheel-matrix.toml`: 10 required + 3 candidate platform tags, duplicate-tag fail-closed (`--self-test` in CI) |
| `release_smoke.py` | Controlled-fixture server smoke: installed entry point or explicit binary over a temp fixture (never the repo root) |
| `wheel-matrix.py` | Canonical wheel-target authority (Plans 265/268): `validate`, `emit-matrix`, `expected-tags`, `self-test`. Enforces the Track A split (`manylinux` container baseline vs `compatibility` maturin policy) and Track B deferred-smoke routing |
| `check-release-workflow.py` | Release-graph guard (Plans 268 Tracks D/E/F, 269): aggregate gates abi-proof + AArch64 glibc/musl + Windows ARM64 qualifiers, no `continue-on-error` on required lanes, explicit QEMU setup + `sh -c`, 3.15 prerelease resolution, `MACOSX_DEPLOYMENT_TARGET 11.0` pin; also runs in routine CI |
| `abi_smoke.py` | Compact stable-ABI fixture (Plan 264): representative native classes/functions through the installed wheel; used by every-minor ABI proof and `MODE=abi-smoke` |
| `qualify-python-wheel-target.sh` | Portable real-device SBC qualification (Plan 266 Track D, rootless): `--wheel PATH` or `--package`/`--version` with real loopback smoke |
| `check-python-types.py` | Installed-wheel typing-fixture runner: `mypy --strict --no-incremental` with the installed-wheel interpreter |

### Conformance corpora (`conformance/`)

Shared Rust/Python test data, validated by
`verify-conformance-matrix.py`; deterministic subsets run in `cargo test`,
expensive/browser/soak/impairment/perf evidence stays manual and fail-closed. See
[testing-and-conformance.md](testing-and-conformance.md).

| File | Content | Routine / manual split |
|------|---------|------------------------|
| `conformance_matrix.toml` | H1 static matrix: 51 `[[matrix]]` entries (resource/method/conditional/range/file-state/version/connection → expected status, body-forbidden, reuse) | Routine: replayed in Rust wire/static suites |
| `corpus.json` | Canonical HTTP-type corpus: methods, status classes, header name/value rules, version parsing; consumed by Rust + Python canonical-conformance suites | Routine |
| `body_corpus.json` | Request-body corpus: policy selection, fixed/chunked accounting, limit enforcement, one-shot, service-declared method bodies, partial-consumption close | Routine |
| `app_server_conformance.toml` | Plan 207 cross-protocol inventory: 55 scenarios (**47 routine**, 8 manual) across H1 TCP/TLS/prebound/Unix, H2 prior/TLS/prebound, H3 QUIC, caller-owned duplex × native/`http_interop`/`tower`/`async_python`/`asgi_fixture` | Routine subset in `cross_protocol_conformance.rs` (19 tests); H1-TLS/H2-TLS/H3/`http-interop`/async/`asgi` owned by their dedicated suites and referenced, not duplicated |
| `http3_qualification.toml` | H3 qualification inventory: `[metadata]` + 17 `[[scenario]]` (protocol/adversarial/lifecycle/interop/config/deps/promotion) | Deterministic part in `http3_runtime.rs`; independent-client/adversarial/impairment/platform via `qualify-http3.sh`; H3 stays experimental |

### Fuzzing (`fuzz/`, 11 targets)

Property-based input fuzzing with per-target seed corpora under
`fuzz/corpus/<target>/`. Invariants: no panics on arbitrary input, no
`..`/`.` in accepted components, no NUL bytes in decoded paths, no
double-decoding, satisfiable ranges within file size, rejections map to valid
`PathRejection` variants. Corpus replay runs in `cargo test`
(`corpus_replay`, `stateful_fuzz_replay`); live `cargo fuzz run <target>` is
manual. See [testing-and-conformance.md](testing-and-conformance.md)
and `../docs/fuzzing.md`.

| Target | What it fuzzes |
|--------|---------------|
| `request_target` | Origin-form classification + confinement handoff + request-head construction |
| `percent_decode` | Single-pass percent decoding (malformed encodings, invalid UTF-8) |
| `path_components` | Normalization + component validation (encoded dot-components) |
| `validate_method` | Method construction/validation + body rejection for read-only methods |
| `range_header` | Range parsing/clamping (suffix, open-ended, start-end, zero-size files) |
| `if_none_match` | ETag comparison (weak/strong, wildcard, comma-separated lists) |
| `platform_component` | Windows checks (reserved names, drive prefixes, alternate data streams) |
| `fuzz_header_block` | `HeaderName`/`HeaderValue`/`HeaderBlock` operations |
| `fuzz_normalize_response` | `StatusCode` validation + response building/normalization + Content-Length reconciliation |
| `fuzz_request_body` | `RequestBody` one-shot state machine |
| `fuzz_directory_buffer` | Listing-buffer behavior + HTML well-formedness (Windows-only; empty harness on Linux/macOS) |

### Benchmarks (`benchmarks/`)

Evidence store, never CI gates. Claims must name a profile (see
`../docs/deployment.md`) + evidence file, with explicit runtime limits, one
excluded warm-up, repeated trials with variance, and raw files retained —
never one best number; absolute RPS/latency never gates PR/routine CI. See
[testing-and-conformance.md](testing-and-conformance.md) for the per-plan
evidence index (Plans 088/109/168/170/227/231–234/240–241).

### Repo-level tests (`tests/`)

Shell harnesses, manual/`deep` only (proxy interop also auto-runs in `deep`
when `caddy`+`nginx` are on `PATH`). See
[testing-and-conformance.md](testing-and-conformance.md).

| Path | What it does |
|------|--------------|
| `tests/installed-binary-qual.sh` | Installed-binary smoke in an isolated env (manual deep check) |
| `tests/lib.sh` | Shared shell helpers for the harnesses |
| `tests/proxy/caddy_interop.sh` / `nginx_interop.sh` | Eggserve behind Caddy/nginx (TLS termination, reuse, header forwarding, timeout alignment, no desync) |
| `tests/proxy/desync_corpus.sh` | Request-smuggling/desync corpus against the proxy front |
| `tests/soak/soak_24h.sh <profile>` | 24h mixed-traffic soak; profiles `unix-reverse-proxy \| unix-direct-https` + `fixtures/` |

### Examples (`examples/` + `crates/eggserve-core/examples/` + `crates/eggserve-server/examples/`)

Executable product demonstrations, indexed by
[`../examples/README.md`](../examples/README.md). Rust examples smoked by
`verify.sh full` (`test-examples.sh`, loopback port `0`); Python static/custom
examples smoked by `test-python-wheel.sh` (port `0`); in-process examples bind
nothing; listener examples bind loopback with port `0`.

| Example | Demonstrates |
|---------|--------------|
| `examples/python_http_server_static.py` | Source-familiar `eggserve.server` static facade (native fast path; loopback, no listings/dotfiles/symlinks) |
| `examples/python_custom_handler.py` | Bounded synchronous `BaseHTTPRequestHandler` (`/health`, `/`, 404; Rust-owned listener/framing) |
| `examples/python_lowlevel_service.py` | Handler-only `eggserve.lowlevel` substrate (buffered + bounded unknown-length stream; `(host, 0)` ephemeral use) |
| `examples/python_async_server.py` (experimental, H1-only) | `eggserve.lowlevel.AsyncServer`: buffered echo + bounded streams, 16-chunk bridges, `max_async_tasks` admission |
| `examples/python_subprocess.py` | Optional `eggserve.subprocess.ServerProcess` lifecycle convenience |
| `examples/python_safe_download.py` | Hardened download via `lowlevel.SecureRoot` + response planning (never re-joined/reopened) |
| `examples/python_https_server.py` | Rust-TLS-backed `ThreadingHTTPSServer` (PEM cert/key) |
| `examples/python_custom_headers.py` | `default_content_type` + ordered safe `extra_response_headers` (final 200s only) |
| `static_server.rs` / `custom_service.rs` / `streaming_service.rs` (core) | Confined `StaticService`; tiny `service_fn` match; transport-independent streams (HEAD/body-forbidden never polls) |
| `application_service.rs` (core) | Plan 197 native contract without static FS (`RequestContext`, `Response`-only return) |
| `caller_owned_stream.rs` (core) / `caller_owned.rs` (server) | Canonical H1 driver over `tokio::io::duplex` (binds nothing) / downstream-neutral direct embedding over loopback TCP |
| `custom_headers.rs` / `https_server.rs` / `primitives.rs` (core) | Static header hooks; rustls HTTPS; security + response-planning primitives with no listener |

### CI jobs (`.github/workflows/ci.yml`, three concurrent jobs)

| Job | What it runs |
|-----|--------------|
| `rust` | Conformance-matrix + topology + release-metadata gates → wheel-target authority → `fmt` → MSRV (`cargo +1.89`) workspace + http2/tls + http3/tls + http-interop/Tower checks → stable clippy + workspace tests → http-interop/Tower qual → excluded Python-crate `cargo check` → `tls` (bin) → http2+tls (core, bin) → http3+tls (core, bin) |
| `supply-chain` | `install-cargo-tools.sh` (pinned audit/deny) then `check-supply-chain.sh` over both lockfiles |
| `python` | Release-metadata preflight → maturin install → `test-python-wheel.sh` (Linux-only routine lane) |

### Docs vs trace records

- Normative user contracts: `docs/` (`security-policy`, `threat-model`,
  `python-http-server-compatibility`, `cli`, `python-api`, `http-primitives`,
  `deployment`, `timeout-reference`, `ops-logging`, `migration-guide`, …).
- Change-trace records (not API docs): `plans/`, `ROADMAP.md`,
  `release/` per-plan closure reports. When docs conflict with
  config/scripts, trust the executable source.

---

## Deep Dive Index

Every subsystem has a dedicated deep-dive document. Use this index to
navigate directly to what you need for a focused review.

### Crates

| Document | Covers |
|----------|--------|
| [crate-topology.md](crate-topology.md) | Plans 211–253 Cargo ownership and dependency boundaries (+254–258 notes) |
| [eggserve-primitives.md](eggserve-primitives.md) | Canonical transport-neutral leaf |
| [eggserve-server.md](eggserve-server.md) | Single mature H1 runtime + `Service` contract (direct authority) |
| [eggserve-static.md](eggserve-static.md) | Sole static/path/filesystem authority (incl. Plan 224 NO-GO) |
| [eggnet-tls.md](eggnet-tls.md) | Plan 212 neutral rustls identity, trust, client-auth, and reload substrate |
| [eggserve-h3.md](eggserve-h3.md) | Plan 220 H3/QUIC package boundary and adapter authority |
| [eggserve-core.md](eggserve-core.md) | Compatibility/composition umbrella — module map, facades, orchestration, error surfaces |
| [eggserve-bin.md](eggserve-bin.md) | CLI binary — `run()` entrypoint, accept loop delegation, argument inventory, signal handling, TLS loading |
| [eggserve-python.md](eggserve-python.md) | Python wheel — `eggserve.server` facade, `eggserve.lowlevel` (+ experimental async), `eggserve.subprocess`, security boundary |

### Security

| Document | Covers |
|----------|--------|
| [path-confinement.md](path-confinement.md) | 6-stage path validation pipeline — parsing, decoding, normalization, component validation, 17 rejection variants |
| [filesystem-confinement.md](filesystem-confinement.md) | `PinnedRoot`, `RootGuard`, descriptor-relative traversal (Unix), handle-relative (Windows), TOCTOU prevention |
| [policy-system.md](policy-system.md) | `StaticPolicy` (+`StaticMetadataPolicy`, `ErrorRepresentationPolicy`), `ResponsePolicy`/`DatePolicy`/denylist, safe defaults, CLI/Python mapping |
| [security-model.md](security-model.md) | Central invariant, 7 defensive layers, attacker model, trust boundaries, platform security |
| [../docs/unsafe-code-policy.md](../docs/unsafe-code-policy.md) | Workspace deny-by-default policy and reviewed FFI/test exceptions |

### HTTP and Runtime

| Document | Covers |
|----------|--------|
| [primitives-api.md](primitives-api.md) | Public facade for embedding — `SecureRoot`, `ResolvedResource`, canonical types, HTTP validation, body primitives |
| [response-planning.md](response-planning.md) | Conditional/range/ETag planning, static validator privacy, HEAD parity, `normalize_response()`, streaming buffer |
| [runtime.md](runtime.md) | `Server`, `ServerBuilder`, `Service` trait, `StaticService`, lifecycle state machine, connection pipeline (incl. final privacy boundary), body ingestion, Plan 249 single-H1-authority rule |
| [http2.md](http2.md) | Hyper-backed opt-in H1/H2 selection, bounded H2 transport policy, ownership checklist, experimental release status |
| [http3.md](http3.md) | Quinn/h3 same-port UDP lifecycle, bounded QUIC/H3 policy, canonical adapters (thin core facade over the H3 authority), experimental qualification boundary |
| [tls.md](tls.md) | rustls-based TLS — PEM loading, PKCS key formats, ALPN, deployment profiles, SNI/mTLS/reload, limitations |

### Operations

| Document | Covers |
|----------|--------|
| [structured-logging.md](structured-logging.md) | Event-based logging (schema v1) over the `eggserve-server::ops` authority (core facade), JSON Lines/text output, operational counters, sanitized fields, log sink types |
| [configuration.md](configuration.md) | `RuntimeConfig`, `ServeConfig`, `Limits` — full field inventory, ownership model, CLI/Python/Rust convergence |
| [error-taxonomy.md](error-taxonomy.md) | 5 error layers — `PathRejection`, `RequestValidationError`, `ServerError`, `ServiceError`, `RequestBodyError` |

### Quality and Process

| Document | Covers |
|----------|--------|
| [testing-and-conformance.md](testing-and-conformance.md) | Rust unit/integration tests, Python suites, 11 fuzz targets, conformance corpora, packaging smoke tests, benchmark/qual evidence |

### Decision Records

| Document | Topic | Status |
|----------|-------|--------|
| [adr-002](adr-002-windows-handle-relative-filesystem.md) | Windows handle-relative filesystem confinement | Accepted |
| [adr-003](adr-003-custom-service-ownership.md) | Custom-service ownership model | Accepted |

---

## How It All Works Together

### Request Lifecycle

```
HTTP Request
    │
    ▼
┌─────────────────────────────────────────────────────┐
│ eggserve-bin: process entry point                   │
│  • CLI argument parsing (args.rs, no clap)          │
│  • Optional TLS identity via eggnet_tls (neutral)   │
│  • Tokio runtime creation                           │
│  • Signal handler registration (shutdown.rs)        │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Direct H1 runtime (eggserve-server) + compatibility │
│ orchestration (eggserve-core::server)               │
│  • Shared RuntimeState admission pool               │
│    (connection semaphore; server-wide file-stream   │
│     semaphore cloned per connection)                │
│  • Compatibility Auto classifies before any Hyper   │
│    service exists; every H1 path delegates to the   │
│    direct driver; core executes H2 only (Plan 249)  │
│  • Optional TLS handshake (feature-gated)           │
│  • H1 strict via the direct driver; optional H2/H3  │
│    via feature-gated glue (H3 uses QUIC)            │
│  • Caller-owned stream entry (no socket required)   │
│  • Lifecycle: Created → Starting → Running →        │
│    Draining → Stopped/Failed                        │
│  • Canonical RequestHead extraction                 │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Canonical driver (server/connection/)               │
│  • Strict serve_http1_connection; optional H1/H2    │
│  • ConnectionContext (TCP, TLS, or caller-owned)    │
│  • TE+CL framing validation (smuggling prevention)  │
│  • Body policy selection (Reject/Buffer/Stream)     │
│  • Body ingestion (timeout, limit, accounting)      │
│  • Handler timeout enforcement                      │
│  • Request → canonical Request envelope             │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Service::call(Request) — one contract               │
│  e.g. StaticService or Python callback handler      │
│                                                     │
│  StaticService pipeline (authority: eggserve-static)│
│  1. Validate method (GET/HEAD only)                 │
│  2. Parse target → ConfinedPath (path confinement)  │
│  3. Resolve via SecureRoot → ResolvedResource       │
│  4. Plan response (conditional, range, ETag)        │
│  5. Stream file / list directory / error            │
│                                                     │
│  Python callback pipeline:                          │
│  1. spawn_blocking → GIL acquire                    │
│  2. Call Python handler with PyRequest              │
│  3. Convert PyResponse → canonical Response         │
│  4. Validate handler response (hop-by-hop, status)  │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│ Response pipeline (runtime owns framing)            │
│  1. Canonical response normalization                │
│     (HEAD suppression, body-forbidden enforcement,  │
│      hop-by-hop stripping, content-length)          │
│  2. Transport-body conversion (to_hyper_response)   │
│  3. Permit release + connection termination         │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
         HTTP Response
```

### Security Layers

Defense in depth across seven layers:

| Layer | What it defends against | Deep Dive |
|-------|------------------------|-----------|
| Path confinement | Traversal, encoding abuse, NUL bytes | [path-confinement.md](path-confinement.md) |
| Policy enforcement | Symlinks, dotfiles, directory listing | [policy-system.md](policy-system.md) |
| Filesystem confinement | Symlink escape, root traversal, TOCTOU | [filesystem-confinement.md](filesystem-confinement.md) |
| Input validation | Double-encoding, method abuse, body framing | [security-model.md](security-model.md) |
| Resource limits | Slowloris, exhaustion, file stream contention | [configuration.md](configuration.md) |
| Response normalization | Hop-by-hop smuggling, content-length manipulation | [response-planning.md](response-planning.md) |
| Sanitized logging | Log injection, path/header leakage | [structured-logging.md](structured-logging.md) |

### Configuration Flow

Configuration is split between runtime-owned (transport) and static-service-owned (filesystem) concerns:

```
CLI flags / Python params / Rust structs
         │
         ▼
┌─────────────────────────────────────────┐
│ Limits (validated subset)               │
│  • connections, streams, timeouts,      │
│    body sizes, parser bounds, listing,  │
│    chunk size                           │
└────────┬───────────────┬────────────────┘
         │               │
         ▼               ▼
┌────────────────┐  ┌────────────────────┐
│ RuntimeConfig  │  │ ServeConfig        │
│ (transport)    │  │ (filesystem)       │
│ • bind addr    │  │ • root directory   │
│ • timeouts     │  │ • static policy    │
│ • TLS          │  │ • file streams     │
│ • keep-alive   │  │ • bind address     │
└────────────────┘  └────────────────────┘
```

---

## Core Library Module Map (`eggserve-core`)

Compatibility aggregate: static/path/filesystem, request/service, and H3
paths are facades over the direct authorities (Plans 214–220; Plan 224
confirms no capability-filesystem crate). Do not treat the deleted
`src/fs`, `src/path`, or `src/mime.rs` as live modules.

| Module | Visibility | Purpose | Stability |
|--------|-----------|---------|-----------|
| `config.rs` | **pub** | `ServeConfig`, `ServeState`, `StartupSummary` (documented orchestration) | Stable-ish |
| `limits.rs` | **pub** | `Limits` — connections, streams, timeouts | Stable-ish |
| `policy.rs` | **pub** | Facade re-exporting `eggserve_primitives::policy` | Stable-ish |
| `ops/` | **pub** | Facade over the `eggserve-server::ops` authority (events/sinks/counters) | Stable-ish |
| `primitives/` | **pub** | Public facade — canonical types for embedding (static/path/filesystem entries re-export `eggserve-static`; deleted `src/fs`, `src/path`, `src/mime.rs` must not return) | Stable |
| `server/` | **pub** | Runtime service boundary: full TLS/H2/H3 `Server`/`ServerBuilder`/`ServerHandle`, `RuntimeConfig` orchestration, `StaticService` composition, H2/listener/proxy/TLS glue; H3 is a thin facade over `eggserve-h3`; H1 paths delegate to the direct driver (Plan 249) | Experimental |
| `tls.rs` | **pub** | TLS transport glue re-exporting the neutral `eggnet-tls` API (feature-gated) | Experimental |

Every production module is in the classified inventory enforced by
`scripts/check-crate-topology.py` (Plan 225 facade closure); new modules
fail the gate until explicitly classified.

---

## Crate Source Structure

### eggserve-core (compatibility/composition umbrella)

```
src/
├── lib.rs                    # module declarations, 3-tier stability model
├── config.rs                 # ServeConfig, ServeState, StartupSummary (documented orchestration)
├── limits.rs                 # Limits — connections, streams, timeouts
├── policy.rs                 # facade re-exporting eggserve_primitives::policy
├── ops/                      # facade over the eggserve-server::ops authority
├── tls.rs                    # TLS transport glue re-exporting neutral eggnet-tls (feature-gated)
├── runtime_limits.rs         # facade re-exporting eggserve_server::runtime_limits
├── response.rs               # compatibility response helpers (pub(crate))
├── primitives/               # compatibility facades — every file re-exports its
│                             # direct authority (eggserve-primitives, eggserve-static,
│                             # or eggserve-server adapters); no second implementation.
└── server/                   # runtime service boundary (experimental):
                              # full TLS/H2/H3 Server/ServerBuilder/ServerHandle,
                              # RuntimeConfig orchestration, StaticService composition,
                              # H2/listener/proxy/TLS transport glue; H3 is a thin
                              # facade over eggserve-h3; H1 delegates to the direct driver
```

### eggserve-bin

```
src/
├── main.rs    # thin fn main() → eggserve_bin::run()
├── lib.rs     # run(), run_cli(argv); neutral paths name the leaf crates directly (Plan 221)
├── args.rs    # manual argument parsing (no clap)
├── shutdown.rs# signal handling (Ctrl+C, SIGTERM, SIGHUP) with broadcast channel
└── tls.rs     # re-export of the neutral eggnet_tls substrate (Plan 221)
```

### eggserve-python (Rust bridge + Python facade)

```
src/
├── lib.rs     # PyO3 module registration; neutral bridge names the leaf crates directly (Plan 221)
└── server/    # bridge submodules: errors, body_bridge, request_bridge, response_bridge,
               # tunnel_bridge, static_responder, sync_handler, runtime, lifecycle,
               # async_handler (experimental; Plan 204 stays Python-side in lowlevel.py)

python/eggserve/
├── __init__.py     # top-level namespace (version, serve_directory, facade classes)
├── __init__.pyi    # type stub for the facade namespace
├── _bin.py         # CLI entry point via native _run_cli
├── __main__.py     # python -m eggserve support
├── _native.pyi     # type stub over the native extension surface
├── server.py       # six-class Rust-runtime compatibility facade
├── server.pyi      # type stub for the facade classes
├── lowlevel.py     # handler-only substrate (+ experimental H1-only async)
├── lowlevel.pyi    # stub for the lowlevel surface
└── subprocess.py   # subprocess lifecycle exports (ServeConfig, ServerProcess)
```

---

## Error Taxonomy

Five distinct error layers, each scoped to a specific subsystem.
See [error-taxonomy.md](error-taxonomy.md).

| Error Type | Scope |
|-----------|-------|
| `PathRejection` | Path parsing (17 variants) |
| `RequestValidationError` | HTTP-level, Python-facing (6 variants) |
| `ServerError` | Server lifecycle (`#[non_exhaustive]`, 10 variants) |
| `ServiceError` | Per-request struct over a private kind (`Internal` / `Rejected(u16)` / `Panic` / `Timeout`; inspect via `is_panic`/`is_timeout`) |
| `RequestBodyError` | Body consumption (`#[non_exhaustive]`, incl. trailer variants) |

---

## Module Visibility Model

| Tier | Modules | Stability |
|------|---------|-----------|
| **Stable** | `primitives` (facade over `eggserve-primitives` + `eggserve-static`), all `primitives::*` submodules | Intended public boundary for embedding consumers |
| **Stable-ish** | `config`, `limits`, `policy`, `ops` | Field shapes may evolve before 1.0 |
| **Experimental** | `server` (all types), `tls`, H2/H3 paths, tunnel/trailer/adapter/listener/proxy surfaces, async Python | API may change without notice |
| **Internal** | `response` and other `pub(crate)` helpers | `pub(crate)` — not part of public API |

---

## Platform Support

| Platform | Status | Security Model |
|----------|--------|----------------|
| **Linux x86_64** (glibc, manylinux_2_17) | Supported-hardened | Descriptor-relative traversal via `statat`+`openat` |
| **Linux aarch64** (glibc, manylinux_2_17) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64; pre-publish proof is native ARM64 Ubuntu (manylinux wheel only) |
| **Linux armv7** (glibc/musl) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64; glibc and musl wheels execute under matching ARMv7 userspaces via QEMU (`sh -c`) |
| **Linux x86_64** (musl, musllinux_1_2) | Supported-hardened | Same descriptor-relative guarantees; musl libc uses the same path |
| **Linux aarch64** (musl, musllinux_1_2) | Supported-hardened | Same descriptor-relative guarantees as Linux x86_64 (musl); pre-publish proof is a native ARM64 Alpine container (musllinux wheel only) |
| **macOS** (x86_64, arm64) | Supported-hardened | Same descriptor-relative guarantees as Linux |
| **Windows x86_64** | Supported-functional | Handle-relative child resolution, reparse-point denial, and directory enumeration are qualified for the executed classes. Two open-descendant root-rename cases remain skipped because NTFS rejects that external path operation; keep Windows for trusted/local content. |
| **Windows arm64** | Supported-functional | Same as Windows x86_64. The `win_arm64` artifact is cross-built and executed natively on the Windows ARM64 hosted runner as a required pre-publication gate; support remains functional (trusted/local content), not hardened. |

---

## Testing Strategy

Multi-layered testing spans the Rust/Python suites, 11 fuzz targets, and
the conformance corpora. See
[testing-and-conformance.md](testing-and-conformance.md) for the full matrix.

| Layer | Location | Scope |
|-------|----------|-------|
| Rust unit tests | `crates/*/src/**/*.rs` (inline `#[cfg(test)]`) | Module-level logic |
| Rust integration tests | `crates/*/tests/*.rs` | Cross-module, live TCP, TLS, parity/authority fixtures (`direct_h1_parity`, `direct_service_convergence`, `static_authority_conformance`, `downstream_embedding`, `tower_compatibility`/`interop_http_tower`/`axum_tower_qualification`, `cross_protocol_conformance`, `tls_identity`, `http2_runtime`, `http3_runtime`) |
| Python test suites | `crates/eggserve-python/tests/test_*.py` | Compatibility facade, TLS, low-level runtime, async bridge/lifecycle (Plans 254/257–258), conformance, body, boundary hardening (`asgi_fixture.py` is the ASGI test fixture; `typing_smoke.py` guards stub fidelity via `check-python-types.py`) |
| Packaging smoke tests | `scripts/test-python-wheel.sh` (authoritative harness) + `crates/eggserve-python/packaging-tests/` (installed-wheel supplement) | Installed-wheel validation |
| Conformance corpora | `conformance/*.toml` + `*.json` (51-entry H1 matrix, 55-scenario cross-protocol inventory with 47-routine subset, 17-scenario H3 inventory) | Shared Rust/Python test data |
| Fuzz targets | `fuzz/fuzz_targets/*.rs` | Property-based input fuzzing (11 targets) |
| Repo-level tests | `tests/` | Proxy interop, soak, installed-binary qual |

---

## Release Process

Release is a manual workflow dispatch. CI is a regression screen, not release certification:

1. Run `./scripts/verify.sh full` (examples, Rust + Python wheel).
2. Run `bash scripts/install-cargo-tools.sh` then `bash scripts/check-supply-chain.sh`.
3. Manually dispatch the release workflow (builds, validates, and publishes via OIDC Trusted Publishing).
4. Production PyPI upload requires the protected `pypi` GitHub Environment.
5. No push/tag/merge automatically publishes.

See [../docs/release-process.md](../docs/release-process.md) for the full procedure.

Historical design records remain in [`../plans/`](../plans/); they are not
required to understand the current runtime or security contract.
