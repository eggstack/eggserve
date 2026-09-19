# Plan 226 — Post-225 release-readiness and metadata corrective

## Purpose

Close the small but real inconsistencies left after Plan 225 without reopening
the crate-ownership campaign.

Plans 217–225 successfully removed duplicate implementation authorities:
canonical request/service/tunnel behavior lives in the direct crates, static
path/filesystem authority lives in `eggserve-static`, H3 lives in
`eggserve-h3`, and `eggserve-core` no longer owns a second security or
protocol implementation. The remaining issues are release/version truth,
MSRV alignment, stale package/architecture descriptions, and remote CI
evidence.

This plan is corrective only. It must not move implementations between crates
or introduce new features.

## Findings motivating this plan

1. The workspace still declares `version = "0.1.2"`, while
   `plans/ROADMAP.md` and `docs/api-stability.md` state that current
   `main` contains intentional pre-1.0 breaking Rust API changes and must
   not ship as another `0.1.x` patch. The next public release must therefore
   be `0.2.0` or later.
2. The workspace still declares `rust-version = "1.88"`. The current
   eggstack Rust baseline is 1.89 for repositories being moved onto the
   common distributable-binary baseline; eggfetch already declares 1.89.
3. `eggserve-core` is accurately a compatibility facade with composition /
   orchestration responsibilities, but its Cargo description still says
   "Security policy, path confinement, and static-serving primitives", which
   incorrectly implies ownership moved out by Plans 217–225.
4. Some architecture/roadmap text still reflects the original monolithic
   three-crate target rather than the current layered workspace.
5. Plan 225 records a full local green validation matrix, but the latest
   GitHub commit has no independently visible remote Actions result through
   the current repository check. Release readiness should require explicit
   remote CI evidence rather than only a local validation record.
6. `eggserve-bin` and `eggserve-python` still use `eggserve-core` for
   extended orchestration. That is not a Plan 225 failure, but the permanent
   role must be documented truthfully so future cleanup work does not
   repeatedly try to remove legitimate composition glue.

## Goals

- Adopt Rust 1.89 as the workspace MSRV and enforce it consistently.
- Prepare and execute the next release-line transition as `0.2.0`, never as
  another `0.1.x` patch.
- Rename/re-document `eggserve-core` conceptually as the compatibility /
  composition umbrella without renaming the crate.
- Correct stale Cargo package descriptions and architecture diagrams.
- Make the current direct-crate ownership model the single documented
  architecture.
- Require a real remote CI result on the corrective commit before closure.
- Preserve all Plan 225 ownership boundaries and support tiers.

## Non-goals

- Do not remove or rename `eggserve-core`.
- Do not migrate remaining orchestration out of `eggserve-core`.
- Do not change H2/H3 support tiers.
- Do not add HTTP features, Python framework semantics, proxy behavior, or new
  crates.
- Do not revisit the Plan 224 capability-filesystem NO-GO decision.
- Do not perform unrelated dependency upgrades.

## Work

### 1. Formalize the role of eggserve-core

Update architecture and public documentation so the crate is described
consistently as:

> Compatibility and composition layer for EggServe's direct primitives,
> runtime, static-serving, TLS, and optional protocol adapters.

Equivalent wording is acceptable, but it must not imply that core owns path
confinement, MIME, canonical request/response types, TLS identity parsing, or
H3 protocol mechanics.

Update at minimum:
- `crates/eggserve-core/Cargo.toml` package description,
- `README.md`,
- `architecture/eggserve-core.md`,
- `architecture/crate-topology.md`,
- `architecture/overview.md`,
- `docs/public-api-boundary.md`,
- `docs/downstream-app-server.md`,
- `plans/ROADMAP.md`.

Document explicitly that:
- keeping orchestration in core is intentional for the current pre-1.0 line;
- first-party frontends may depend on core for full composed-server behavior;
- new low-level consumers should prefer the direct crates;
- removing/deprecating core requires a separate future migration plan.

Do not label the remaining orchestration as an implementation blocker.

### 2. Correct the roadmap architectural target

Replace the obsolete early three-crate target with the actual current target:

```text
crates/
  eggserve-primitives/  # canonical transport-neutral HTTP/security values
  eggserve-server/      # generic H1/runtime/service/tunnel authority
  eggserve-static/      # static service + filesystem confinement authority
  eggnet-tls/           # neutral TLS identity/trust/client-auth substrate
  eggserve-h3/          # optional experimental H3/QUIC adapter
  eggserve-core/        # compatibility/composition umbrella
  eggserve-bin/         # CLI
  eggserve-python/      # excluded PyO3 wheel crate
```

Ensure no live architecture document still describes core as the sole owner
of path confinement/static serving/canonical HTTP.

### 3. Raise MSRV to Rust 1.89

Change:
- workspace `rust-version` from 1.88 to 1.89,
- CI explicit MSRV toolchain lanes,
- documentation claiming 1.88,
- packaging/release metadata or scripts that assert 1.88.

Keep the exact release compiler pin (currently 1.98.1) separate from MSRV:
- MSRV = oldest supported compiler,
- release compiler = exact compiler used to build published artifacts.

Validation must include:
- `cargo +1.89 check --workspace --all-targets`,
- feature-gated 1.89 checks for the supported build combinations that are
  currently exercised at MSRV,
- package verification at the declared MSRV where feasible.

Do not raise beyond 1.89 in this plan unless a dependency proves that 1.89 is
already insufficient; if that occurs, stop and document the concrete
dependency requirement rather than silently increasing the floor.

### 4. Execute the required 0.2.0 version transition

The current main line must not publish as 0.1.3.

Update the synchronized package version authority to 0.2.0 and propagate it
through:
- workspace package version,
- intra-workspace `path + version` constraints,
- excluded `eggserve-python` package version and path-version constraints,
- Python package metadata,
- release scripts/configuration,
- lockfiles where package versions are recorded,
- examples/tests/docs that intentionally assert the package version.

Use the existing release metadata synchronization/check scripts rather than
manual one-off edits where those scripts own the value.

Add/update release notes and migration guidance so the reason for the minor
transition is explicit:
- intentional pre-1.0 Rust API changes accumulated on `main`,
- direct-crate ownership convergence,
- compatibility paths retained where documented,
- no implication that H2/H3 experimental tiers were promoted.

The version change itself is release metadata, not a support-tier change.

### 5. Package metadata cleanup

Review Cargo descriptions for all workspace crates after the ownership
campaign.

At minimum verify:
- `eggserve-core`: compatibility/composition wording;
- `eggserve-h3`: actual H3 transport adapter, not merely dependency boundary;
- `eggserve-static`: static service + confinement authority;
- `eggserve-server`: generic HTTP runtime/service authority;
- `eggserve-primitives`: canonical neutral values;
- `eggnet-tls`: neutral TLS substrate.

Descriptions should tell a crates.io user what the crate owns now, not what it
owned before Plans 211–225.

Do not broaden keywords/categories solely for discoverability unless they
remain accurate.

### 6. Remote CI evidence gate

After the corrective commit is pushed, require a real GitHub Actions result
for the exact commit SHA.

Required remote jobs:
- normal Rust CI / topology / conformance,
- supply-chain audit over both lockfiles,
- Python wheel job,
- any release-metadata synchronization gate that normally runs on main.

If no workflow run is created:
1. inspect workflow triggers and repository Actions configuration;
2. distinguish "connector cannot see runs" from "GitHub did not run CI";
3. obtain evidence from the repository's actual Actions/checks surface;
4. fix workflow triggering only if CI genuinely failed to start.

Do not mark Plan 226 closed solely from local tests.

### 7. Final release-readiness verification

Run the current full routine matrix after the version/MSRV corrections:

- `python3 scripts/verify-conformance-matrix.py`
- `python3 scripts/check-crate-topology.py`
- `python3 scripts/check-python-release-metadata.py`
- `cargo fmt --all -- --check`
- `cargo +1.89 check --workspace --all-targets`
- MSRV feature checks for `http2,tls` and `http3,tls` where supported
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`
- `cargo test --workspace`
- excluded Python crate checks
- `scripts/check-supply-chain.sh`
- `scripts/verify-cargo-packages.sh --mode all`
- doc tests and examples
- both dist builds
- Python wheel build/install/test suite

Record the exact corrective SHA and the remote CI run/check URLs or IDs in a
new release evidence document.

## Security invariants

This plan must preserve:
- one path/filesystem authority in `eggserve-static`;
- one canonical request/service/tunnel authority across
  `eggserve-primitives` / `eggserve-server`;
- one H3/QUIC adapter authority in `eggserve-h3`;
- one neutral TLS identity/trust authority in `eggnet-tls`;
- no eggfetch/eggress product dependency;
- rustls security floor >= 0.23.45 in every distributed closure;
- daily advisory scanning;
- default graph free of H3/QUIC;
- current H1/H2/H3 support tiers unchanged.

## Tests / structural gates to add or update

1. Metadata sync check rejects a future `0.1.x` release candidate from this
   main line.
2. CI/MSRV scripts assert 1.89 rather than 1.88.
3. Topology/documentation checks, if they encode crate descriptions or
   ownership language, are updated to the composition terminology.
4. Package verification proves every published crate's `path + version`
   constraint resolves at 0.2.0.
5. Python release metadata check proves Rust/Python package versions remain
   synchronized where required.
6. Remote CI evidence for the exact closing SHA is recorded.

## Migration / compatibility

No user-facing code migration should be required solely because of Plan 226
beyond the already-documented 0.2.0 migration guidance.

The version transition is intentionally a pre-1.0 minor release because
current main already contains breaking stable-Rust API changes. Do not create
additional breaking API changes merely because the version is changing.

## Rollback

- Do not roll back to a 0.1.x release number after publication preparation has
  begun. Fix forward on the 0.2.x line.
- If Rust 1.89 exposes a genuine build regression, correct the code/dependency
  issue; only raise MSRV again with explicit evidence and documentation.
- If remote CI fails, fix the failure before release; do not replace remote
  evidence with a local-only waiver.
- Documentation wording may be revised, but do not reclassify core as an
  implementation authority unless a future plan intentionally moves
  implementation back into it.

## Acceptance criteria

Plan 226 is complete only when all of the following are true:

- workspace MSRV is 1.89 and CI enforces it;
- next release metadata is 0.2.0 everywhere it must be synchronized;
- no current release script can accidentally publish this line as 0.1.x;
- `eggserve-core` Cargo/docs accurately call it a compatibility/composition
  layer;
- the roadmap's architectural target matches the actual seven-crate layout;
- all workspace crate descriptions match current ownership;
- full local validation passes;
- a remote GitHub Actions/check suite passes for the exact closing SHA;
- release evidence records that SHA and remote result;
- Plan 225 ownership/security invariants remain unchanged.
