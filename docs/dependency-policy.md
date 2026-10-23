# Dependency Policy

The release smoke fixture uses only Python's standard library. The direct
server crate is the HTTP/1 file-stream conversion boundary for the extracted
layers; compatibility core retains the advanced protocol adapters and their
transport-specific conversions.

Plans 211–214 add a checked crate topology. `eggserve-primitives` is the canonical
leaf and intentionally has only the small `bytes`/`futures-util` dependencies
needed for owned bytes and streams. `eggserve-server` owns
Hyper/Tokio transport and depends on primitives, never on static serving or
the compatibility and composition umbrella. `eggserve-static` owns filesystem-specific
behavior and depends on primitives plus server. `eggserve-core` is the
compatibility and composition umbrella. `eggnet-tls` owns neutral
rustls identity/trust/client-auth/reload policy and depends only on rustls and
rustls-pki-types at runtime. `eggserve-h3` owns the direct Quinn/H3/H3-Quinn
production dependency set and is optional from the core facade. See
[`architecture/crate-topology.md`](../architecture/crate-topology.md).

## Rules

Every dependency must have an explicit purpose. The following rules apply to all dependencies:

- **No HTTP client stack without a plan** — HTTP client dependencies require an explicit plan and feature gate. Plan 223 explicitly keeps the shared outbound H1 CONNECT wire primitive outside eggserve: no neutral CONNECT crate, no HTTP client stack, and no eggfetch/eggress product dependency enters any eggserve graph; eggserve owns only inbound server-side tunnel acceptance (Plans 199/216)
- **No web framework dependency in the initial milestones** — no actix-web, axum, warp, etc.
- **No templating dependency for generated directory listings** — directory listings use static HTML
- **No default TLS dependency** — TLS dependencies are optional, behind the `tls` feature flag, and not included in the default build
- **Feature flags must isolate optional surfaces** — optional dependencies are behind feature flags
- **Security-critical parsing dependencies require review** — any dependency that handles HTTP parsing, path resolution, or encoding must be reviewed before adoption
- **Shared versions are declared once** — a dependency used by more than one workspace member takes its version from the root `[workspace.dependencies]` table (see below)

## Shared dependency specifications (Plan 315)

A dependency that more than one workspace member consumes takes its version
from the root `[workspace.dependencies]` table. Members inherit the version
with `workspace = true` and keep the crate-specific parts in their own
manifest:

```toml
# crates/eggserve-core/Cargo.toml
rustls = { workspace = true, optional = true, default-features = false, features = ["std", "ring"] }
```

Rules:

- **Only genuinely shared dependencies belong in the table.** A dependency
  consumed by exactly one crate stays in that crate's manifest: `phf`
  (static-only), `thiserror` (core-only), the H3/QUIC stack
  (`h3`/`h3-quinn`/`quinn`, H3-only), and the dev-only `axum`/`serde`
  fixtures.
- **The table carries versions, not feature policy.** Cargo *adds* a member's
  `features` to the inherited set instead of replacing it, so the intentionally
  divergent per-crate feature sets (for example `tokio-rustls` with `["ring"]`
  where a crate needs it and without where it does not) stay in the crates
  that need them.
- **`default-features` is a per-crate decision, stated in the table only when
  every member agrees.** A member-level `default-features = false` on top of a
  table entry that leaves the defaults enabled is ignored by Cargo (and
  warned about), so the table entry and the members must agree. The workspace
  table therefore carries `default-features = false` for `rustls` and
  `tokio-rustls`, and members that need the crate's default feature set
  restate `default-features = true`.
- **The excluded Python crate stays explicit.** `crates/eggserve-python` is
  excluded from the workspace and cannot use the workspace table at all. It
  keeps every specification — including the `rustls` `0.23.46` floor —
  literal in its own manifest, and it keeps its own `Cargo.lock`.
- **Path dependencies stay local.** The `eggserve-*` crates are published path
  dependencies with per-crate versions; they are not table entries.
- **Consolidation never changes resolution.** Moving a version into the table
  must leave both `Cargo.lock` files byte-identical and the resolved feature
  sets unchanged; if it would not, the dependency stays explicit.

## Initially allowed categories

The following dependency categories are approved for initial development:

| Category | Dependencies | Purpose |
|----------|-------------|---------|
| Async runtime | `tokio` | Event loop and async primitives |
| HTTP server | `hyper`, `hyper-util`, `http-body`, `http-body-util` | HTTP protocol handling; `http-body` is the `Body` trait for response-completion tracking (Plan 164) |
| Ecosystem interop | `http` (optional, `eggserve-server/http-interop`) | Direct `http` message types for loss-aware canonical adapters (Plans 200/276); transitive via Hyper today, direct so public adapters do not rely on re-exports |
| Ecosystem interop | `tower-service` (optional, activated by `eggserve-server/tower`); `tower-layer` (optional declared, no longer activated — dev-dependency for tests, direct dependency of downstream `Layer` authors) | Minimal Tower `Service`/`Layer` traits for middleware/app adapters (Plans 200/276/296); full `tower` never required, never in default builds |
| Buffer types | `bytes` | Efficient byte buffer management |
| Streaming | `futures-util` | Async stream utilities for file streaming bodies |
| Date formatting | `httpdate` | HTTP date formatting for Last-Modified headers |
| Compile-time map | `phf` (`eggserve-static` only) | Perfect hash function map for MIME type lookup; Plan 225 removed the leftover core edge so the table lives once in the static authority |
| CLI parsing | manual (no clap) | Manual argument parsing in `eggserve-bin` |
| Error derive | `thiserror` | Derive macro for Error types |
| Python bindings | `pyo3` 0.29.2 (eggserve-python only) | PyO3 bindings for Python wheel; pinned above the current RustSec advisories affecting 0.24.x |
| TLS | `rustls` 0.23.46+ (optional, feature-gated) | TLS termination; floor is security-sensitive (Plan 218, RUSTSEC-2026-0285; raised to 0.23.46 by Plan 318 for RUSTSEC-2026-0346, -0347, -0348, -0349) |
| TLS | `tokio-rustls` (optional, feature-gated) | Async TLS stream wrapping |
| TLS | `rustls-pki-types` (optional, feature-gated) | PEM certificate and key parsing |
| Neutral TLS substrate | `eggnet-tls` | Bounded server identity, SNI, WebPKI mTLS, trust/CRL parsing, and atomic reload; no application or transport dependencies |
| HTTP/3 transport | `eggserve-h3` → `h3`, `h3-quinn`, `quinn` (optional, `http3` feature) | Experimental HTTP/3/QPACK server semantics, Quinn Tokio QUIC transport, and the rustls QUIC crypto adapter; no default/H1/H2 graph impact |
| WebSocket interop fixture (dev-only) | `tokio-tungstenite` (dev-dependency, tests only) | Plan 199 Track I: proves generic tunnel handoff sufficient for downstream WS codec (handshake via EggServe, framing over `TunnelIo`); never enters production `eggserve-core`/`eggserve-bin` graphs |
| Windows filesystem | `windows-sys` (optional, Windows-only, feature-gated) | Handle-relative filesystem operations for Windows hardening |
| Unix syscalls | `rustix` (Unix-only) | `eggserve-static`: descriptor-relative filesystem confinement (`fs`) plus socket-activation fd validation; `eggserve-core`: listener accept/socket validation (`net` only, plus ungated `rustix::io::Errno`) — Plan 219/225, no `fs` feature in core; no service-manager crate in default or minimal builds |

### Tokio feature ownership

| Crate | Tokio features (production) | Notes |
|-------|---------------------------|-------|
| `eggserve-primitives` | none | No Tokio/Hyper/filesystem edge; transport-neutral canonical layer |
| `eggserve-server` | `macros`, `net`, `time`, `io-util`, `fs`, `sync` | Generic transport runtime and async file-body bridge; no static edge |
| `eggserve-static` | target-gated `rustix` on Unix | Descriptor-relative filesystem, MIME, and static service |
| `eggserve-core` | `macros`, `net`, `time`, `fs`, `io-util`, `sync` | No `signal`, no `rt-multi-thread` in default |

| `eggserve-bin` | `macros`, `net`, `signal`, `time`, `sync` | Signal handling for graceful shutdown |
| `eggserve-python` | `rt-multi-thread`, `net`, `io-util`, `sync`, `time` | Python GIL scheduling requires multi-thread |

## Notes

- The dependency graph is intentionally layered: `eggserve-primitives` owns
  canonical values, `eggserve-server` owns generic HTTP transport,
  `eggserve-static` owns filesystem/MIME behavior, and `eggserve-core` keeps
  the compatibility and composition umbrella. The CLI and Python crates add
  only their frontend/runtime requirements.
- `tokio`, `hyper`, `hyper-util`, `http-body`, `http-body-util`, and `bytes` provide the
  HTTP/1 transport and body pipeline. Manual CLI parsing avoids a broad CLI
  framework dependency.
- `futures-util` and `httpdate` support streaming and HTTP dates; the
  compile-time MIME map (`phf`) lives once in `eggserve-static` (Plan 225).
- TLS dependencies are optional and feature-gated. Windows filesystem support
  is likewise target-gated; platform-only dependencies do not enter the
  default Unix graph.
- `eggnet-tls` is an independently reusable workspace crate. Its production
  graph contains only `rustls` and `rustls-pki-types`; Tokio, tokio-rustls,
  HTTP, QUIC, proxy, tracing, filesystem watching, and application policy stay
  in consuming projects.
- H3 dependencies are optional and feature-gated behind `http3` (which also
  enables `tls`). `eggserve-h3` owns the direct declarations and pins the
  selected coordinated versions in `Cargo.lock`; the core compatibility facade
  consumes only that package. The boundary is covered by the same
  `cargo audit`/`cargo deny`
  gates as the default graph. Plans 188, 190, 192, 193, 194, and 195 close with H3
  experimental because
  independent-client and adversarial QUIC evidence was unavailable on the
  qualification host; the manual script remains the release path. Plan 192
  freezes the latest released stack (`h3` 0.0.8 / `h3-quinn` 0.0.10 / Quinn
  0.11.11 / rustls 0.23.x) and closes `BLOCKED`: `hyperium/h3#338` has no
  released fix and the `hyperium/h3#262` stream-drop remainder is unresolved,
  so H3 stays experimental until a later readiness update (see
  `release/plan-192-http3-dependency-readiness.md`). Plan 193 retains the
  tier on the unchanged candidate; Plan 194 adds the per-stream
  producer-timeout correction without changing the tier, and Plan 195
  correctively qualifies that bound without changing the tier. No fork or vendored H3
   patch is permitted to force a supported label. Plan 213 records the package
   boundary and qualification inventory in
   `release/plan-213-http3-quic-isolation-qualification.md`. Plan 218 raises
   the rustls floor inside this coordinated set to 0.23.45 (RUSTSEC-2026-0285)
   without changing the H3 tier or the remaining pinned versions. Plan 318
   raises the floor again to 0.23.46 (RUSTSEC-2026-0346, -0347, -0348, -0349),
   again without changing the H3 tier or any other pinned version.
- Tokio features are owned narrowly: the core library does not enable signal
  handling or a multi-thread runtime; the CLI owns signals and uses a
  current-thread runtime, while Python enables a bounded multi-thread runtime
  for GIL scheduling.
- The default product is a hardened static server with reusable HTTP/security
  primitives; unused client and application-framework dependencies are not
  part of the default graph.
- No dependency is added without updating this document
- `cargo audit` and `cargo deny` run in the routine CI supply-chain job using
  the pinned versions from `scripts/install-cargo-tools.sh`. Because
  `eggserve-python` is excluded from the root workspace, CI invokes
  `scripts/check-supply-chain.sh`, which checks both `Cargo.lock` files and
  applies this same `deny.toml` to both manifests.

## Security-sensitive version floors (Plan 218)

Advisory response can require minimum versions newer than the broad semver
declarations elsewhere in this document. When that happens, the floor is set
explicitly — once in the root `[workspace.dependencies]` table for the
workspace members, and literally in the manifest of any crate that cannot use
that table — so a future lock regeneration cannot legitimately resolve below
the patched line:

- **Both lockfiles are distributed security boundaries.** The root workspace
  `Cargo.lock` and the excluded Python wheel `crates/eggserve-python/Cargo.lock`
  ship in distributed artifacts and are audited and policy-checked together by
  `scripts/check-supply-chain.sh`. A floor must land in both closures; the
  Python manifest carries its own `rustls` constraint for exactly this reason.
- **rustls-family floors are security-sensitive.** RUSTSEC-2026-0285 requires
  `rustls >= 0.23.45`, and RUSTSEC-2026-0346, -0347, -0348, and -0349 require
  `rustls >= 0.23.46`. The `0.23.46` caret floor is declared once in the root
  `[workspace.dependencies]` table and inherited by every direct `rustls`
  constraint in the workspace (`eggnet-tls`, `eggserve-h3`, `eggserve-core`,
  and the `eggserve-server`/`eggserve-bin` dev-dependencies); the excluded
  Python manifest declares the same floor literally because it cannot use the
  workspace table. `tokio-rustls` inherits its `0.26` version from the same
  table. Do not roll a rustls floor back: if a regression appears in a patched
  release, move forward to a later patched release or disable the affected
  optional feature while investigating.
- **The floor is what protects a consumer, not the resolution.** Every
  lockfile in this repository already resolves `rustls 0.23.46`, but a
  consumer who writes `rustls = "=0.23.45"` in their own manifest can
  unify to the vulnerable release alongside these crates unless these crates
  refuse to accept it. That refusal *is* the floor, which is why the floor
  is a manifest requirement rather than a lockfile fact.
- **A static guard must protect the floor.** The supply-chain/topology gates
  must fail if any constraining manifest — including the excluded Python
  crate, the one place the workspace table cannot help — declares `rustls`
  below `0.23.46` or drops it while declaring the `tls`/`http3` features.
- **Sibling repositories still declare bare rustls floors (Plan 222
  follow-up).** Eggress (`rustls = "0.23"`) and eggfetch-core
  (`rustls = "0.23"` optional) currently resolve 0.23.45 in their lockfiles
  but do not enforce the patched floor, so a fresh resolve could pick a
  pre-patch rustls. Raising those floors is a change in each repository, not
  here; coordinate rustls/rustls-webpki/tokio-rustls updates rather than
  letting them drift (see `architecture/eggnet-tls.md`).
- **H3 keeps a coordinated version set.** `eggserve-h3` owns `h3` / `h3-quinn` /
  `quinn` plus the rustls floor above; the set moves together and the H3 tier
  stays experimental regardless of patch bumps (see the H3 note above).
- **The TLS stack stays rustls/ring-only.** `native-tls` and `openssl-sys` are
  banned in `deny.toml` and must not enter any closure. The alternate rustls
  crypto provider (`aws-lc-rs`/`aws-lc-sys`) is intentionally *not* banned
  there: dev-only `rcgen` test-cert generation pulls it into cargo-deny's
  feature-unified graph even though every production (`-e no-dev`) graph is
  ring-only. Production ring-only is verified with
  `cargo tree -e no-dev -p eggserve-bin --features tls` (zero `aws-lc`,
  `openssl`, `native-tls` matches) rather than with a deny rule that cannot
  distinguish dev unification from production linkage.

## Release validation tool versions

CI and release validation install these cargo subcommands from the checked-in
`scripts/install-cargo-tools.sh` script before invoking them. The versions are
deliberately pinned and the script fails if the installed executable reports a
different version.

| Tool | Version | Install command |
|------|---------|-----------------|
| `cargo-audit` | `0.22.2` | `cargo install cargo-audit --version 0.22.2 --locked --force` |
| `cargo-deny` | `0.19.0` | `cargo install cargo-deny --version 0.19.0 --locked --force` |

Run the shared installer locally with:

```bash
bash scripts/install-cargo-tools.sh
```

## Automated enforcement

`cargo-deny` is configured via `deny.toml` at the workspace root. It checks:

- **Advisories** — known vulnerabilities in dependencies
- **Licenses** — only permissive licenses allowed (MIT, Apache-2.0, BSD, ISC, Unicode-DFS-2016, Zlib)
- **Bans** — multiple versions of the same crate produce warnings (retained,
  not forced to single-version); wildcard version requirements are denied;
  `native-tls` and `openssl-sys` are banned outright (Plan 218, rustls/ring-only)
- **Sources** — only crates.io registry allowed; no git dependencies

To run locally:
```bash
bash scripts/install-cargo-tools.sh
cargo audit --version
cargo deny --version
bash scripts/check-supply-chain.sh
```

Routine CI runs the shared root/Python audit and deny checks in a dedicated,
self-contained supply-chain job. Maintainers can reproduce that job locally
with `scripts/install-cargo-tools.sh` followed by
`scripts/check-supply-chain.sh`. The release preflight repeats the same
closure check before building wheels. A scheduled daily workflow
(`.github/workflows/advisory-scan.yml`, Plan 218) re-runs the same gates
without requiring a push or pull request so newly published RustSec advisories
are detected promptly; it performs no builds, tests, or releases and holds no
credentials.

The `audit.toml` at the workspace root configures `cargo audit` defaults. The `deny.toml` configures `cargo deny`.
