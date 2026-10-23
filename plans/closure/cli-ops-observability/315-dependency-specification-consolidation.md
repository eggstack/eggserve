# CLI, Ops, and Observability Milestone 315 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/315-dependency-specification-consolidation.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `dfc9d2d` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Commit `96b9a46` — the implementation: the root `[workspace.dependencies]`
  table, the seven member manifests converted to `workspace = true`, the
  layered-package script fix, and the `AGENTS.md` / skill /
  `docs/dependency-policy.md` updates.

## 1. Executive finding

Shared dependency versions are now declared **once**. The root manifest
carries a `[workspace.dependencies]` table; the seven workspace members
inherit from it; and the advisory-sensitive `rustls` floor is stated in one
place instead of six.

The hard acceptance criterion holds: **all three lockfiles are
byte-identical** before and after, and the resolved package set and every
resolved feature edge are unchanged. The milestone also corrected a factual
error in its own plan that would have changed the resolved feature set of
every member if followed literally, and it uncovered a latent breakage in the
release-preparation script that the change would otherwise have introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| A `[workspace.dependencies]` table exists in the root manifest | `Cargo.toml:26-73` | pass |
| The workspace members inherit from it | All seven member manifests use `workspace = true` | pass |
| Both lockfiles are byte-identical before and after | `sha256sum` of `Cargo.lock`, `crates/eggserve-python/Cargo.lock`, and `fuzz/Cargo.lock` before and after; `git diff --stat -- '*Cargo.lock'` empty | pass |
| No dependency added, removed, or version-changed | `cargo metadata --no-deps` declared-spec diff across all seven members: empty | pass |
| The resolved package set is unchanged | `cargo tree --workspace -e no-dev` diff: empty (87 packages) | pass |
| The resolved **feature** set is unchanged | `cargo tree --workspace -e features` diff: empty; `cargo tree -e features -i rustls` diff: empty, per crate and workspace-wide | pass |
| The `rustls` `0.23.45` floor is never rolled back | The floor is in the root table and inherited by every member; the excluded Python crate keeps an explicit spec; the resolved version stays `0.23.46` | pass |
| The excluded Python crate is unaffected | `crates/eggserve-python/Cargo.toml` diff is empty; `cargo tree --manifest-path crates/eggserve-python/Cargo.toml -e no-dev` diff is empty | pass |
| The fuzz crate is unaffected | `fuzz/Cargo.toml` diff is empty; `fuzz/Cargo.lock` byte-identical | pass |
| The MSRV lane still resolves the same features | `cargo +1.89 check --workspace --all-targets` and `cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower`: clean, no warnings | pass |
| The release-preparation gates still pass | `bash scripts/verify-cargo-packages.sh --mode all`: "Layered crates passed local-registry package verification" | pass |

## 3. Production implementation evidence

### 3.1 The workspace table

The root manifest gained a `[workspace.dependencies]` table carrying the
shared specifications: `rustls`, `tokio-rustls`, `rustls-pki-types`, the
HTTP/1 transport set (`hyper`, `hyper-util`, `http`, `http-body`,
`http-body-util`, `httpdate`), the async/streaming set (`tokio`,
`futures-util`, `bytes`), `rustix`, the Tower trait set (`tower-service`,
`tower-layer`), and the shared dev-dependency set (`rcgen`, `tempfile`,
`proptest`, `libc`).

Dependencies used by exactly one crate stayed local on purpose: `phf`
(static), `thiserror` (core), `h3`/`h3-quinn`/`quinn` (h3), `axum` (server
dev), `serde`/`serde_json`/`tokio-tungstenite` (core dev), and every
`eggserve-*` path dependency — the published crates carry per-crate versions
that must not be unified.

### 3.2 The members

All seven workspace members now inherit their shared versions. The
crate-specific parts stay local, because Cargo *adds* member features to the
inherited set rather than replacing it: `features`, `optional`, and
`default-features` remain in each member manifest.

### 3.3 The `default-features` correction — the material deviation

The plan stated that "Cargo's workspace inheritance does **not** carry
`default-features`; a crate that needs `default-features = false` must state
it explicitly." That is backwards, and following it literally would have
changed the resolved feature set of every member.

Cargo **does** inherit `default-features` from the table. The failure mode is
the opposite one: when a table entry leaves `default-features` unspecified
(defaults **on**) and a member asks for `default-features = false`, the
member-level request is **ignored** and Cargo emits

```
warning: `default-features` is ignored for <dep>, since `default-features`
was not specified for `workspace.dependencies.<dep>`; overriding workspace
`default-features` to false requires Rust 1.99+ and the 2024 edition
```

Overriding to `false` requires Rust 1.99 and edition 2024; this repository is
edition 2021 with MSRV 1.89, so that override would not have been honored on
the MSRV lane at all.

The resolution: the table states `default-features` explicitly wherever the
members do not agree on the default set — `rustls` and `tokio-rustls` both
carry `default-features = false` — and the members that need the default
feature set restate `default-features = true`, which Cargo honors on every
supported toolchain. A member restating `default-features = false` against a
table entry that already says `false` is then a consistency statement rather
than an override.

### 3.4 The lockfile is not the guard — the feature tree is

`Cargo.lock` records versions, not features, so a feature regression would
leave the lockfile byte-identical. The real guard for this milestone is the
resolved feature tree, which was captured before and after and diffed empty
for the workspace, for the dev-inclusive tree, for `-e features`, for
`-i rustls` workspace-wide, and per crate.

### 3.5 The layered-package script

`scripts/verify-cargo-packages.sh` builds a **synthetic** root manifest for
its layered local-registry verification. After this change that synthetic
root had no `[workspace.dependencies]` table, so the member manifests could
not resolve `workspace = true` and `cargo generate-lockfile` failed before any
build:

```
error inheriting `rustls` from workspace root manifest's `workspace.dependencies.rustls`
Caused by: `workspace.dependencies` was not defined
```

The script now extracts the real root manifest's `[workspace.dependencies]`
table and writes it into the synthetic root, so the layered build resolves
exactly what the real workspace resolves — including the `rustls` security
floor.

## 4. Verification executed

| Command | Result |
|---|---|
| `sha256sum` of the three lockfiles, before and after | identical |
| `git diff --stat -- '*Cargo.lock'` | empty |
| `cargo metadata --no-deps` declared-spec diff (7 members) | empty |
| `cargo tree --workspace -e no-dev` diff | empty (87 packages) |
| `cargo tree --workspace` (dev-inclusive) diff | empty |
| `cargo tree --workspace -e features` diff | empty |
| `cargo tree -e features -i rustls` diff (workspace and per crate) | empty |
| `cargo tree --manifest-path crates/eggserve-python/Cargo.toml -e no-dev` diff | empty |
| `cargo check --workspace --all-targets` | clean, no warnings |
| `cargo check --workspace --all-targets --features http2,tls` | clean |
| `cargo check --workspace --all-targets --features http3,tls` | clean |
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | clean |
| `cargo metadata --manifest-path fuzz/Cargo.toml --locked` | clean |
| `cargo +1.89 check --workspace --all-targets` | clean, no warnings |
| `cargo +1.89 check -p eggserve-server --all-targets --no-default-features --features tower` | clean, no warnings |
| `bash scripts/verify-cargo-packages.sh --mode all` | "Layered crates passed local-registry package verification" |
| `python3 scripts/check-crate-topology.py` | exit 0 |

## 5. Invariant review

| Invariant | Result |
|---|---|
| The `rustls` floor is never rolled back | holds — declared once in the root table, inherited by every member, restated literally in the excluded Python crate |
| No dependency is added, removed, or version-changed | holds — the declared-spec diff and the package-tree diff are both empty |
| The excluded crate keeps explicit specifications | holds — `crates/eggserve-python/Cargo.toml` is unchanged |
| Both lockfiles stay byte-identical | holds — all three lockfiles unchanged |
| The fuzz crate keeps its own lockfile and specifications | holds — `fuzz/Cargo.toml` unchanged, `fuzz/Cargo.lock` byte-identical |
| The MSRV lane resolves the same features | holds — `cargo +1.89 check` is clean and warning-free on the workspace and the Tower lane |

## 6. Failure and recovery review

Not applicable: this milestone changes manifest metadata only. There is no
runtime behavior, no cancellation path, and no restart path.

The failure mode that mattered was a *resolution* failure — the layered
package script failing before any build — and it is covered by the
`scripts/verify-cargo-packages.sh --mode all` gate, which is now green.

## 7. Migration and compatibility review

- No public API change and no semver event. The published manifests
  materialize the same dependency requirements and the same feature sets.
- The published crates carry the *same* resolved dependency graph, so a
  consumer's build is unchanged.
- The `rustls` floor remains effective for every consumer, including those
  who depend on the excluded Python wheel.

## 8. Security review

- The `rustls` `0.23.45` floor (RUSTSEC-2026-0285) is the supply-chain
  security floor for this repository. It remains in effect in every
  constraining manifest.
- `docs/dependency-policy.md` gained a *static guard must protect the floor*
  rule and a *Shared versions are declared once* rule.
- The floor's list was corrected: `docs/dependency-policy.md` previously
  omitted `eggserve-h3` from the list of manifests that constrain `rustls`,
  and the plan's own inventory table described `eggserve-server`'s
  declaration as a normal dependency when it is a **dev**-dependency. The
  accurate set is five workspace members — `eggnet-tls`, `eggserve-h3`,
  `eggserve-core`, and the `eggserve-server` and `eggserve-bin` dev
  dependencies — plus the excluded crate.

## 9. Documentation and operations

- `AGENTS.md` — the toolchain note now says the floor is declared once in
  the root `[workspace.dependencies]` table and inherited by every member,
  while the excluded Python crate keeps an explicit spec, and that it must
  never be rolled back.
- `.opencode/skills/eggserve-dev/SKILL.md` — the same statement in the
  agent-facing supply-chain section and in the Plan 222 bullet.
- `docs/dependency-policy.md` — a new *Shared dependency specifications
  (Plan 315)* section describing the layout and six rules; the
  *Security-sensitive version floors* section rewritten; and the rustls list
  corrected.
- `plans/implementation/cli-ops-observability/315-dependency-specification-consolidation.md` —
  the recorded material deviation.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low | `docs/dependency-policy.md`'s allowed-categories table lists `windows-sys` (Windows-only, feature-gated), but no member manifest declares it — the Windows FFI is hand-written `extern "system"` declarations. | Pre-existing drift, left alone: the entry reads as an approved category rather than a live dependency, and the Windows FFI is out of this milestone's scope. |
| low | `cargo publish --dry-run` was not run in this environment, so the published-manifest materialization of `workspace = true` entries is inferred from Cargo's documented behavior rather than proven by a dry run. | The layered local-registry verification (`scripts/verify-cargo-packages.sh --mode all`) exercises the same materialization path against a real registry and is green. |
| low | The plan's §3.4 inventory needed two corrections (see §8 above). | Corrected in the implementation plan's recorded deviation and in `docs/dependency-policy.md`. |

## 11. Roadmap disposition

**Milestone closed.**

Shared dependency versions are declared once; the advisory floor is stated in
one place and inherited everywhere it applies; the excluded crate keeps its
explicit specifications; all three lockfiles are byte-identical; and the
resolved package **and feature** sets are unchanged, which is the guard that
actually matters for a metadata-only milestone.

The roadmap's Milestone 315 moves `ready` → `closed`.

## 12. Registry updates

`plans/registry.md`:

- The CLI, ops, and observability row's current milestone becomes 315,
  closed, with this closure record linked.
- The dependency-ready implementation plans row for 315 moves `ready` →
  `closed`, with this closure record and the recorded deviation.

`plans/subsystems/cli-ops-observability-roadmap.md`:

- Milestone 315 moves `ready` → `closed`, with this closure record linked and
  the recorded deviation named.
