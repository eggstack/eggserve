# CLI, Ops, and Observability Milestone 318 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/318-rustls-floor-and-testpypi-staging.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `ce457ae` (Plans 313, 315, and 316 closed)

Implementation commits or pull requests:

- Commit `4a38141` — the floor raise in both manifests, the new gate rule
  `check_plan318_rustls_floor` with its self-test mutations, and the
  narrative references in `AGENTS.md`, the skill, `docs/dependency-policy.md`,
  `docs/migration-guide.md`, `architecture/eggserve-h3.md`, and
  `architecture/eggnet-tls.md`.

## 1. Executive finding

The `rustls` floor was the one dependency constraint in this repository that
was declared security-sensitive and simultaneously **under-enforced**, and
the policy that describes it claimed a mechanical guard that **did not
exist**.

Two findings, one of them a documentation defect that was hiding a real gap:

1. **The floor did not protect consumers.** Every lockfile in the repository
   already resolved `rustls 0.23.46`, so the advisory scan was clean. But the
   *manifest* floor sat at `0.23.45`, and a manifest floor is what refuses to
   unify with a consumer's own pin. A consumer writing `rustls = "=0.23.45"`
   in their own manifest could still resolve the vulnerable release alongside
   these crates, because these crates accepted it. The floor was raised to
   `0.23.46`, which closes that window.
2. **`docs/dependency-policy.md` demanded a guard that was not there.** The
   policy states: *"The supply-chain/topology gates must fail if any
   constraining manifest — including the excluded Python crate, the one place
   the workspace table cannot help — declares `rustls` below the floor or
   drops it while declaring the `tls`/`http3` features."* No such check
   existed in any script. There is now one: `check_plan318_rustls_floor`.

The resolution did not move. All three lockfiles are byte-identical before
and after, and the resolved feature sets are unchanged. What changed is what
the crates *accept*.

The TestPyPI staging path was established rather than assumed: the GitHub-side
configuration is complete and the workflow's TestPyPI job is correctly wired,
and the one remaining step is an account-side registration on the TestPyPI
service that this repository does not control.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Raise the floor in the workspace table | `Cargo.toml`: `rustls = { version = "0.23.46", default-features = false }` | pass |
| Raise the floor in the excluded Python crate | `crates/eggserve-python/Cargo.toml`: `rustls = { version = "0.23.46", ... }` | pass |
| The resolution must not move | All three lockfiles byte-identical before and after (`sha256` comparison) | pass |
| The feature set must not move | `cargo tree -e features` for the workspace and for the excluded crate: unchanged | pass |
| The floor is declared once | `check_plan318_rustls_floor` reads the root `[workspace.dependencies]` table and requires every inherited member to inherit it | pass |
| The excluded crate cannot be helped by the table | The same rule requires the excluded crate's literal requirement to state the floor | pass |
| A static guard must protect the floor | Gate rule `check_plan318_rustls_floor`, with five self-test mutations | pass |
| The TestPyPI staging path is executable | The `testpypi` GitHub environment exists; the release workflow's `publish-testpypi` job targets `https://test.pypi.org/legacy/` with `id-token: write` and `environment: testpypi` | pass (GitHub side) |
| The remaining account-side step is recorded | §10 below | recorded, not performed |

## 3. Production implementation evidence

### The floor raise

Two manifests changed, and only the version literal in each:

| Manifest | Before | After |
|---|---|---|
| `Cargo.toml` (root `[workspace.dependencies]`) | `rustls = { version = "0.23.45", default-features = false }` | `rustls = { version = "0.23.46", default-features = false }` |
| `crates/eggserve-python/Cargo.toml` | `rustls = { version = "0.23.45", default-features = false, features = ["std", "ring"] }` | `rustls = { version = "0.23.46", default-features = false, features = ["std", "ring"] }` |

Every other crate inherits the table. `eggserve-static` and
`eggserve-primitives` do not constrain `rustls` at all, which is why the
`eggserve-static` 0.5.1 patch (Plan 319) is independent of this milestone.

### Resolution neutrality, proved

| Lockfile | SHA-256 before | SHA-256 after |
|---|---|---|
| `Cargo.lock` | `77205600f03a526954409aff76fc298ed7d9234c69d9f6e51dcb9adb0f49e81b` | identical |
| `crates/eggserve-python/Cargo.lock` | `14eca41692e438b523abaea76a2e63360dd1aa7fd197d0b966ae82416fded062` | identical |
| `fuzz/Cargo.lock` | `67919cb3662da3df544aecb1320c3eead34ee605388ea4abac2290d53d813620` | identical |

`cargo tree -e features` for the workspace and for the excluded crate is
byte-identical before and after. `cargo metadata`'s package set is unchanged.

This is the expected result and the point of the change: the lockfiles were
already on the patched release, so only the *declared* floor moved. A
consumer's resolution does not change; what changes is that a consumer who
pins `=0.23.45` can no longer unify with these crates.

### The missing static guard

`check_plan318_rustls_floor` enforces:

1. the root `[workspace.dependencies]` table declares `rustls` at the floor;
2. the table does not *drop* `rustls`;
3. every other manifest either inherits (`workspace = true`) or states the
   floor literally — the excluded Python crate is the case that matters,
   because it cannot use the table;
4. every manifest the rule names exists.

Its five self-test mutations each prove it fails closed: a regressed floor in
the excluded crate, a regressed floor in the workspace table, a table that
drops `rustls`, a missing manifest, and the passing case. The rule was also
proved against the live tree by deliberately regressing the excluded crate's
floor (`0.23.45`) and the table's floor in turn; both were caught and the
tree was restored.

### The TestPyPI staging path

The GitHub-side configuration is complete:

- the `testpypi` GitHub environment exists in `eggstack/eggserve`, alongside
  `pypi` and `release`;
- the release workflow's `publish-testpypi` job downloads the wheel-set
  artifact, excludes the evidence `MANIFEST` from the upload set, and
  publishes through `pypa/gh-action-pypi-publish` against
  `https://test.pypi.org/legacy/` with `id-token: write` and
  `environment: testpypi`.

What is missing is the PyPI-side registration, which is an account action on
the TestPyPI service and cannot be performed from this environment. §10
records it precisely.

## 4. Verification executed

| Check | Command | Result |
|---|---|---|
| Lockfile neutrality | `sha256sum` over all three lockfiles, before and after | identical |
| Feature-tree neutrality | `cargo tree -e features` for the workspace and the excluded crate | unchanged |
| Package-set neutrality | `cargo metadata --format-version 1` | unchanged |
| Python crate builds | `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | clean |
| Fuzz crate builds | `cargo check --manifest-path fuzz/Cargo.toml --bins --locked` | clean |
| Workspace clippy | `cargo clippy --workspace --lib --bins --tests -- -D warnings` | clean |
| Workspace tests | `cargo test --workspace` | exit 0 |
| Format | `cargo fmt --all -- --check` | clean |
| Topology gate | `python3 scripts/check-crate-topology.py` | exit 0 |
| Topology self-test | `python3 scripts/check-crate-topology.py --self-test` | 89 checks passed |
| The new rule fails closed | A deliberately regressed floor in the excluded crate and in the table | both caught |
| Advisory scan | `cargo audit` over all three lockfiles | clean |
| Hosted CI | CI run `39927249552` on `main` (`4a38141`) | all five jobs green |

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| The resolved dependency set does not change | Lockfiles byte-identical; `cargo metadata` package set unchanged | holds |
| The resolved feature set does not change | `cargo tree -e features` unchanged for the workspace and the excluded crate | holds |
| The floor is declared once | The gate rule reads the root table and requires inheritance | holds |
| The excluded crate keeps its own specification and lockfile | `crates/eggserve-python/Cargo.toml` states `0.23.46` literally; its lockfile is separate and audited separately | holds |
| No publication happens as a result of this plan | No publish command was run | holds |
| The floor never moves backwards | The gate rule fails on any lower floor | holds |

## 6. Failure and recovery review

No failures. The one surprise was that the floor raise was *already* in
effect in every lockfile, which is why the advisory scan had been clean and
the gap had gone unnoticed: a clean `cargo audit` says the *resolution* is
safe, not that the *declaration* is. That distinction is now stated in
`docs/dependency-policy.md` under "The floor is what protects a consumer,
not the resolution".

## 7. Migration and compatibility review

A consumer that pins `rustls = "=0.23.45"` can no longer use these crates.
That is the intended effect — `0.23.45` carries RUSTSEC-2026-0346, -0347,
-0348, and -0349 — and it is recorded in `docs/migration-guide.md` under
Plan 318, which states that such a consumer must move to `0.23.46` or later.

No public Rust API changes. No behavior changes. The dependency *set* and
*feature set* are unchanged for every consumer who resolves normally.

## 8. Security review

The four advisories the floor now excludes are:

| Advisory | Severity | Fixed in |
|---|---|---|
| RUSTSEC-2026-0346 | 4.0 (medium) | `rustls` ≥ 0.23.46 |
| RUSTSEC-2026-0347 | 2.3 (low) | `rustls` ≥ 0.23.46 |
| RUSTSEC-2026-0348 | 3.7 (low) | `rustls` ≥ 0.23.46 |
| RUSTSEC-2026-0349 | 3.7 (low) | `rustls` ≥ 0.23.46 |

The floor protects the consumer only while it is *declared*; the new gate
rule is what keeps it declared. No new dependency was introduced, and the
allowed dependency categories are unchanged.

## 9. Documentation and operations

- `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md` — the floor and the
  advisories it corresponds to.
- `docs/dependency-policy.md` — the floor in the table, the narrative, the
  "floor protects a consumer" principle, and the guard that now exists.
- `docs/migration-guide.md` — the Plan 318 section, including the consumer
  compatibility event.
- `architecture/eggserve-h3.md`, `architecture/eggnet-tls.md` — the floor
  each names.

## 10. Unresolved findings

1. **The TestPyPI trusted publisher is still not registered on the
   TestPyPI account.** This is the one remaining step of the plan's work
   package C, and it is an account action on a service this repository does
   not control. The GitHub side is complete and verified: the `testpypi`
   environment exists and the workflow job is wired. The remaining step is
   to register the trusted publisher on the TestPyPI account for
   `eggstack/eggserve`, workflow `release.yml`, environment `testpypi` — the
   same three fields the production `pypi` publisher uses, pointed at
   TestPyPI. After that, the staging sequence is
   `gh workflow run release.yml -f publish_target=testpypi`, then
   `gh workflow run release.yml -f publish_target=pypi`.
   This closure record does **not** claim a staging run that did not happen.
2. **The floor raise creates a republication that is owed.** The manifests
   of `eggnet-tls`, `eggserve-h3`, `eggserve-core`, `eggserve-server`, and
   `eggserve-bin` now materialize `rustls = "0.23.46"`, so their published
   artifacts still carry the old floor. `eggnet-tls` 0.2.0 in particular
   was published with the `0.23.45` floor and its content now differs from
   the local tree. That republication is a separate release line with its
   own plan; it is not absorbed here.
3. **`docs/dependency-policy.md`'s allowed-categories table still lists
   `windows-sys`**, which no manifest declares. Pre-existing drift,
   unrelated to this milestone, not actioned here.

## 11. Roadmap disposition

`plans/subsystems/cli-ops-observability-roadmap.md` gains **Milestone 318 —
Rustls security floor and its mechanical guard**, recorded as closed in the
milestone status table.

## 12. Registry updates

`plans/registry.md`: the Plan 318 row is added to the implementation-plan
table, and the next milestone number remains 320.
