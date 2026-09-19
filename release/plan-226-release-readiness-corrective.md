# Plan 226 — Post-225 release-readiness corrective evidence

## Decision

**CLOSED.** The Plan 225 follow-up corrective is implemented without
reopening the crate-ownership campaign: no implementation moved between
crates and no feature was added. All Plan 225 ownership and support-tier
invariants are preserved.

## Corrective commit

- SHA: `4b7ebd272ab0e4d7ec3bb64cf7c6df7f61153394`
  (`Implement Plan 226 post-225 release-readiness corrective (0.2.0 line,
  MSRV 1.89, composition docs)`)
- Remote CI run:
  `https://github.com/eggstack/eggserve/actions/runs/35427457482`
  (run `35427457482`, conclusion `success`,
  `headSha` == corrective SHA, updated `2026-09-19T06:59:58Z`)
- Per-job check results for the exact SHA (via the repository
  commits/check-runs surface):
  - `rust`: `completed` / `success`
  - `python`: `completed` / `success`
  - `supply-chain`: `completed` / `success`

The run was created by the push itself (the repository's `ci.yml`
`push: branches: [main]` trigger); no workflow-trigger fix was needed,
and no local-only waiver was used.

## What changed

- **Version line `0.1.2` → `0.2.0`:** workspace package version, all
  intra-workspace `path + version` constraints, excluded
  `eggserve-python` package version and path-version constraints, Python
  package metadata (`pyproject.toml`), release packaging script
  (`scripts/verify-cargo-packages.sh`, including the staged-workspace
  template, registry-rewrite patterns, and crate-file name), and both
  lockfiles (surgically: only the 7 workspace package entries per
  lockfile; no transitive dependency upgrades).
- **MSRV `1.88` → `1.89`:** workspace `rust-version`, CI explicit MSRV
  toolchain lane and all three MSRV `cargo check` invocations,
  `docs/toolchain-support.md`, `AGENTS.md`, the `eggserve-dev` skill,
  and the packaging-script template. The exact release compiler pin
  (`1.98.1`) is unchanged and stays separate from MSRV.
- **`eggserve-core` formalized** as the compatibility and composition
  layer for the direct primitives, runtime, static-serving, TLS, and
  optional protocol adapters: new Cargo description and crate docs,
  plus `README.md`, `architecture/eggserve-core.md` (new Composition
  role section), `architecture/crate-topology.md`,
  `architecture/overview.md`, `docs/public-api-boundary.md`,
  `docs/downstream-app-server.md`, `docs/dependency-policy.md`,
  `docs/extension-contract.md`, and `plans/ROADMAP.md`. Keeping
  orchestration in core is documented as intentional for the pre-1.0
  line; frontends may depend on core for composed-server behavior; new
  low-level consumers prefer the direct crates; removal needs a separate
  migration plan.
- **Roadmap target corrected:** the obsolete early three-crate sketch is
  marked superseded (retained for history only) and replaced with the
  actual eight-crate layered layout.
- **Stale descriptions corrected:** `eggserve-h3` (transport adapter,
  not merely dependency boundary), `eggserve-static` (service +
  confinement authority), `eggserve-server` (runtime and service
  authority), `eggserve-primitives` (canonical transport-neutral
  values). `eggnet-tls`, `eggserve-bin`, and `eggserve-python`
  descriptions were already accurate.
- **Architecture pruning:** `architecture/overview.md` source-structure
  section no longer lists the deleted core `src/fs`, `src/path`,
  `src/mime.rs`, `primitives/canonical/`, or `server/http3/` modules;
  binary/Python bridge descriptions match the Plan 221 leaf-direct
  layout.
- **Migration/release notes:** `docs/migration-guide.md` gains a Plan
  226 section (metadata-only transition, no code migration beyond the
  already-documented guidance); `docs/release-contract.md`,
  `docs/release-process.md`, and `SECURITY.md` (now `0.2.x` supported)
  reflect the `0.2.0` line.

## Structural gates

- `scripts/check-python-release-metadata.py` now rejects a `0.1.x`
  workspace version on this line (fails preflight with an explicit
  `0.2.0`-or-later message) in addition to the existing sync checks.
- `scripts/verify-cargo-packages.sh --mode all` proves every published
  crate's `path + version` constraint resolves at `0.2.0` through the
  local-registry layered packaging pass.

## Local validation (corrective tree, before push)

- `verify-conformance-matrix.py`, `check-crate-topology.py`,
  `check-python-release-metadata.py` (version `0.2.0`): pass
- `cargo fmt --all -- --check`: pass
- `cargo +1.89 check --workspace --all-targets` (default,
  `http2,tls`, `http3,tls`): pass
- `cargo clippy --workspace --lib --bins --tests -- -D warnings` plus
  all six feature-gated lint lanes: pass
- `cargo test --workspace`: 1955 passed, 3 ignored (76 suites)
- `core http2,tls`: 1137 passed; `bin http2,tls`: 141 passed;
  `bin tls`: 141 passed; `core http3,tls`: 1150 passed;
  `bin http3,tls`: 141 passed
- excluded Python crate `cargo check --locked`: pass;
  `cargo test --doc -p eggserve-core`: 4 passed;
  `cargo check -p eggserve-core --examples`: pass
- dist builds (`-p eggserve-bin`, default and `tls`): pass
- `check-supply-chain.sh` (both lockfiles): advisories/bans/licenses/
  sources ok
- `verify-cargo-packages.sh --mode all`: layered crates passed at
  `0.2.0`
- `test-examples.sh`: Rust example smoke checks passed
- `test-python-wheel.sh` (CPython 3.14, maturin 1.14.1):
  `eggserve-0.2.0-cp311-abi3` wheel built, installed, smoke-checked,
  804 Python tests `OK`

## Acceptance

- [x] workspace MSRV is 1.89 and CI enforces it
- [x] release metadata is 0.2.0 everywhere synchronized
- [x] metadata sync gate rejects a future 0.1.x candidate on this line
- [x] `eggserve-core` Cargo/docs call it a compatibility/composition layer
- [x] roadmap target matches the actual layered layout
- [x] all workspace crate descriptions match current ownership
- [x] full local validation passes
- [x] remote GitHub Actions suite passes for the exact closing SHA
      (run 35427457482, all three jobs `success`)
- [x] this document records that SHA and remote result
- [x] Plan 225 ownership/security invariants unchanged (topology gate
      green; H1/H2/H3 tiers unchanged; rustls floor `>= 0.23.45`;
      default graph free of H3/QUIC; no eggfetch/eggress dependency)
