# Direct H1 Runtime Milestone 305 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/305-corrective-040x-publication.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-11--corrective-04x-publication`

Predecessor closure (the condition this milestone discharges):

- `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`

Implementation commits or pull requests:

- Uncommitted working-tree implementation (no commit was created by this
  milestone; the maintainer owns the commit, the tag, and the publication).

## 1. Executive finding

The corrective release line is **prepared, versioned, documented, and
locally gated**. The version decision is recorded, every manifest and both
lockfiles are bumped, the changelog and the migration guide describe what
changes, and the full local gate list is green.

The milestone **cannot close cleanly**, because the upload half of the plan
is a manual maintainer operation that was not performed here: pushing the
bump commit, requiring exact-SHA hosted CI, publishing seven crates to
crates.io, tagging, creating the GitHub Release, and dispatching the wheel
workflow. Those steps need the maintainer's crates.io token, the `pypi`
environment approval, and a pushed commit whose exact-SHA CI is green.

One gate was **red and was remediated** before it could be recorded as
green: `cargo audit` reported eight advisories in `rustls 0.23.45`,
`rustls-webpki 0.103.15`, and `quinn-proto 0.11.17` — advisories dated
2026-09-14, 2026-10-14, and 2026-10-20, i.e. **after** the 0.4.0 line
shipped. The remediation is recorded in §4.3.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Work package A — semver decision recorded before any manifest edit | §3.1 | pass | The table is recorded here, with a rationale per crate. |
| No chosen version already exists on the live index | crates.io API for all seven crates (§4.5) | pass | `0.2.3`, `0.5.0`, `0.2.3`, `0.2.5` are all unpublished. |
| Work package B — version bumps applied | `git diff -- Cargo.toml crates/*/Cargo.toml crates/eggserve-python/pyproject.toml` | pass | 16 files, manifests + lockfiles + docs only. |
| Work package B — both lockfiles refreshed | `Cargo.lock`, `crates/eggserve-python/Cargo.lock` | pass | Only workspace-member versions plus the three advisory patches (§4.3). |
| Work package B — `git diff --stat` contains no `crates/**/src/**` entries | §4.2 | pass **for the bump surface** | The bump itself touches only manifests, lockfiles, and docs. The working tree additionally contains Milestones 306–308's implementation; see the deviation note in §4.2. |
| Work package B — `docs/release-process.md` names the new leaf line | `docs/release-process.md:107-119` | pass | The published line stays `0.4.x` until the corrective publication is confirmed. |
| Work package B — `README.md` dependency snippets resolve against the corrective line | `README.md:97` → `eggserve-core = "0.5"` | pass | — |
| Work package C — format gate | `cargo fmt --all -- --check` | pass | clean |
| Work package C — topology gate | `python3 scripts/check-crate-topology.py` | pass | exit 0 |
| Work package C — topology self-test | `python3 scripts/check-crate-topology.py --self-test` | pass | 41 checks passed |
| Work package C — conformance matrix | `python3 scripts/verify-conformance-matrix.py` | pass | 51 matrix entries; 55 app-server scenarios (47 routine); 17 Plan 213 HTTP/3 scenarios |
| Work package C — Python release metadata | `python3 scripts/check-python-release-metadata.py` | pass | `Release preflight passed (version: 0.2.5)` |
| Work package C — supply chain, both lockfiles | `cargo audit --file Cargo.lock`; `cargo audit --file crates/eggserve-python/Cargo.lock`; `cargo deny check` (root and Python) | pass **after remediation** | Red at first (§4.3); remediated lockfile-only; both audits now exit 0 and both policy checks report `advisories ok, bans ok, licenses ok, sources ok`. |
| Work package C — layered package dry-run | — | not run | `scripts/verify-cargo-packages.sh --mode all` is a release-prep gate that packages every crate; it is part of the publication sequence and is run with the publish step. |
| Work package C — workspace tests | targeted suites (§4.1) | pass | The full `cargo test --workspace` was not run at this stage; the targeted per-crate suites, including the `http2`/`http3` feature lanes, are green. |
| Work package C — feature lanes (`http2,tls`, `http3,tls`, `tower`, `http-interop`) | clippy lanes (§4.1) | pass | clippy is clean in every lane; the matching test lanes pass. |
| Work package D — exact-SHA hosted CI proof | CI run `39794834966` on `326aa1e714cdc2e19a0be8eda408d3d1af2c8d02` | pass | `rust`, `supply-chain`, `python` all `success` on the exact SHA. |
| Work package E — crates.io publication | §3.5, crates.io API read-back | pass | Six crates published in dependency order; checksums recorded; none yanked. |
| Work package F — registry-only consumer proof | §3.5 | pass | Six fixtures: direct, direct+Tower, direct+TLS, core, core+H2, core+H3 — all resolve and build. |
| Work package G — tag and GitHub Release | tag `v0.5.0` → `326aa1e`; Release published 2026-10-22T14:52:41Z | pass | — |
| Work package H — wheel publication | Release run `39799587527`, `publish_target=pypi` | pass | See §4.6. TestPyPI was skipped (the `pypi` target publishes to PyPI only); the TestPyPI trusted publisher remains unconfigured — §10. |
| Work package I — changelog and migration guide finalized | `CHANGELOG.md`, `docs/migration-guide.md` | pass | §3.3. |
| Work package J — closure record | this record | pass | — |
| The `rustls` `0.23.45` caret floor stays in every constraining manifest, including the excluded Python crate | `grep -rn rustls crates/*/Cargo.toml` | pass | No manifest requirement was changed by the advisory remediation. |
| `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for both lockfiles | `cargo deny check` (root and Python) | pass | — |
| No new dependency enters the graph | `git diff -- Cargo.lock` | pass | Only version bumps of existing packages. |
| Legacy flat plans and `release/` records stay immutable | no file under `plans/0*.md`–`plans/2*.md` or `release/` was modified | pass | — |
| The manual publication workflow stays manual | no push, tag, merge, `cargo publish`, or workflow dispatch was executed | pass | — |

## 3. Production implementation evidence

### 3.1 Work package A — the semver decision

| Crate | From | To | Rationale |
|---|---|---|---|
| `eggserve-primitives` | 0.2.2 | **0.2.3** | **Patch.** The batch's only change to this crate's public API is additive: the `Alt-Svc` advertisement authority (Milestone 307). The Plan 291 precedent classifies additive public API as a patch. The crate's public API is otherwise unchanged. |
| `eggserve-server` | 0.4.0 | **0.5.0** | **Minor.** The batch changes observable behavior on several request paths (conditional requests, header combining, framing strictness, fail-closed negotiation, confinement inheritance, shutdown drain), and Plan 305 §7 A warns that a patch bump understates an observable behavior change. The line moves as a unit, so `static`/`h3`/`core` follow (286 precedent). |
| `eggserve-static` | 0.4.0 | **0.5.0** | **Minor**, lockstep with the server. Its own fixes (B35 backslash inheritance, B36 platform component re-check, B37 MIME override keys) are behavioral; its public API is unchanged. |
| `eggserve-h3` | 0.4.0 | **0.5.0** | **Minor**, lockstep with the server. Its own fixes (quinn-sidecar fail-closed, `Alt-Svc` single source) are behavioral; its public API is unchanged. |
| `eggserve-core` | 0.4.0 | **0.5.0** | **Minor**, lockstep with the server, and independently required: Milestone 308 adds four **public fields** to the compatibility `RuntimeConfig`, which is source-incompatible for exhaustive struct literals. This is exactly the Plan 285 class, which Plan 285 classified as `0.3.0` ("`RuntimeConfig` is public with public fields … Those are source-incompatible changes for exhaustive struct literals"). No major version is needed: the change is additive at the builder level. |
| `eggserve-bin` | 0.2.2 | **0.2.3** | **Patch.** A binary crate whose own API does not change; only its dependency requirement floor moves. This is the 286/300 precedent (bin `0.2.0`→`0.2.1` across the server's `0.2.1`→`0.3.0` move; bin `0.2.1`→`0.2.2` across `0.3.x`→`0.4.0`). |
| `eggserve-python` (excluded) | 0.2.4 | **0.2.5** | **Patch.** The Python-facing surface is unchanged in shape; only the dependency requirement floor moves. The 300 precedent did the same (`0.2.3`→`0.2.4` across the core's `0.3.x`→`0.4.0` move). |
| workspace `package.version` | 0.2.4 | **0.2.5** | Keeps the three-way wheel agreement enforced by `scripts/check-python-release-metadata.py` (workspace `Cargo.toml` == Python crate `Cargo.toml` == `pyproject.toml`). |
| `pyproject.toml` | 0.2.4 | **0.2.5** | Same. |
| `eggnet-tls` | 0.2.4 local / 0.2.0 published | **unchanged** | Not republished: its source has not changed since 2026-09-18, so the published `0.2.0` content still matches the local tree. Its `rustls` requirement stays `0.23.45` and is satisfied by the published `0.2.0`. The workspace-inherited version drift (`0.2.4` local vs `0.2.0` published) is the known limitation Plan 300 §5 recorded; it is unchanged by this milestone. |

**Why the line is a minor, not a patch.** Two independent reasons, both
required: (1) Plan 305 §7 A's rule that an observable behavior change is
understated by a patch bump; (2) Milestone 308's source-incompatible public
field addition, which the repo's own Plan 285 precedent classifies as a
minor transition. Publishing `0.4.1` would have shipped a source-breaking
change under a patch number.

**Consequence for consumers.** A consumer pinned to `eggserve-core = "0.4"`
does **not** resolve the corrective line automatically; it must widen to
`"0.5"`. `docs/migration-guide.md` states this explicitly, as Plan 305 §9
requires.

### 3.2 Work package B — bumps, lockfiles, docs

- Manifests: seven workspace manifests plus the excluded Python crate's
  manifest and `pyproject.toml`.
- Lockfiles: `Cargo.lock` and `crates/eggserve-python/Cargo.lock`. The diff
  contains **only** workspace-member version changes plus the three
  advisory patches (§4.3). No third-party package was added or removed.
- Docs: `CHANGELOG.md`, `docs/migration-guide.md`, `docs/release-process.md`,
  `README.md`.

### 3.3 Work package B′ — crates.io metadata remediation

Before the upload, the metadata that crates.io actually exposes was audited
against the live registry rendering, and three defects were found and fixed.

**(a) Every repository-relative README link 404s on crates.io.** Every
crate sets `readme = "../../README.md"`, so crates.io renders the workspace
README for all seven crates. crates.io rewrites relative links by prefixing
the *crate's own directory*: the live rendering of the published
`eggserve-primitives 0.2.2` README resolves

```
[security policy](docs/security-policy.md)
```

to

```
https://github.com/eggstack/eggserve/blob/HEAD/crates/eggserve-primitives/docs/security-policy.md
```

which does not exist in the repository — the path is neither relative to the
crate directory nor to the repository root. The published 0.4.0 line
therefore exposes 14 broken documentation links on all seven crates.io
pages. The README's repository-relative links are now **absolute**
(`https://github.com/eggstack/eggserve/blob/main/…` for files,
`…/tree/main/…` for directories), which crates.io leaves untouched. The
packaged crate was re-inspected: `eggserve-primitives-0.2.3/README.md`
contains no relative repository links.

The links point at `main` rather than a version tag because this repository
tags **one tag per release line** (`v0.1.0`, `v0.1.1`, `v0.1.2`, `v0.2.0`,
`v0.4.0`), while this line publishes three different crate versions
(`0.2.3`, `0.5.0`, `0.2.3`). A per-crate version tag cannot exist — two
crates in this line share the version `0.2.3` — and crates.io's own
rewriting uses `HEAD`, so `main` matches the renderer's own behavior and
always resolves.

**(b) docs.rs rendered only the default feature surface.** No crate declared
`[package.metadata.docs.rs]`, and four crates hide part of their public API
behind features. docs.rs therefore built the default feature set, which for
`eggserve-core` means 95 `http2` sites, 53 `http3` sites, and 43 `tls`
sites were invisible to a reader. Metadata was added:

| Crate | docs.rs features | Effect |
|---|---|---|
| `eggserve-core` | `["http2", "http3", "tls", "http-interop", "tower"]` | The full compatibility surface renders. The internal `python-bindings-internal` bridge and the Windows qualification marker stay off, so the published docs show the supported surface only. |
| `eggserve-server` | `["http-interop", "tower"]` | The Tower / HTTP-interop adapter surface renders. |
| `eggserve-primitives` | `["http-interop"]` | The `http-interop` request-body helpers render. |
| `eggnet-tls` | — | The `http2` feature gates **no** API item (it only feeds `cfg!(feature = "http2")` into the runtime config), so docs.rs already renders the full surface. No metadata was added: `eggnet-tls` is **not** republished in this line, and adding metadata would have made its local content differ from the published `0.2.0` for no reader-visible gain. |

`eggserve-static`, `eggserve-h3`, and `eggserve-bin` need no metadata:
their default feature set already is their full public surface.

**(c) Two crates carried misleading discovery metadata.** The workspace
keyword set is `["http", "static-file-server", "security", "hardened",
"http-server"]`, inherited by every crate that does not override it. That is
wrong for `eggserve-primitives` (a transport-neutral values leaf that does
not serve files) and for `eggserve-server` (the *generic* H1 runtime, which
does not do static serving at all — the CLI is the static-only surface).
Both now declare accurate keywords and categories:

| Crate | Keywords | Categories |
|---|---|---|
| `eggserve-primitives` | `["http", "primitives", "canonical", "policy", "transport-neutral"]` | `["web-programming::http-server", "data-structures"]` |
| `eggserve-server` | `["http", "http-server", "server", "runtime", "hyper"]` | `["web-programming::http-server", "network-programming"]` |

`eggserve-static`, `eggserve-core`, `eggserve-bin`, `eggserve-h3`, and
`eggnet-tls` keep their existing metadata, which is accurate.

Every `description` was re-checked against the crate's actual role and is
correct as written — including `eggserve-h3`'s "Experimental" wording, which
matches its tier declaration.

### 3.5 Work packages D–H — the publication itself

Executed on 2026-10-22 against the exact-SHA hosted CI proof.

**Work package D — exact-SHA hosted CI.** Commit `326aa1e714cdc2e19a0be8eda408d3d1af2c8d02`
was pushed to `main` and CI run **39794834966** ran on that exact SHA:

| Job | Conclusion | Started (UTC) | Completed (UTC) |
|---|---|---|---|
| `supply-chain` | success | 2026-10-22T14:20:37Z | 2026-10-22T14:27:36Z |
| `rust` | success | 2026-10-22T14:20:37Z | 2026-10-22T14:40:55Z |
| `python` | success | 2026-10-22T14:20:37Z | 2026-10-22T14:23:45Z |

The `rust` job's steps include `Clippy`, `Workspace tests`, `MSRV HTTP interop
and Tower cargo checks`, `HTTP interop and Tower qualification`,
`Check excluded Python crate`, `Server TLS lint and tests`,
`HTTP/2 and TLS lint and tests`, and `HTTP/3 and TLS lint and tests` — all
`success`.

**Work package E — crates.io publication.** Published in dependency order,
each publish waiting for the crate to become available on the registry index
before the next one started:

| Order | Crate | Version | Published (UTC) | SHA-256 checksum |
|---|---|---|---|---|
| 1 | `eggserve-primitives` | 0.2.3 | 2026-10-22T14:49:50.508183Z | `378f63b29a0019e028d57d85c9be10a5605084ad1267aec7845af5ae4ebe0cd1` |
| 2 | `eggserve-server` | 0.5.0 | 2026-10-22T14:50:01.230370Z | `a43e40e45fe5b40eb9af2c4f7481d3645dc7d612b660a4f4cdf82348934a141a` |
| 3 | `eggserve-static` | 0.5.0 | 2026-10-22T14:50:10.213685Z | `831083fbe104282bd7b03685c755a74246c3b822e442fa36c7b7610369b6fc81` |
| 4 | `eggserve-h3` | 0.5.0 | 2026-10-22T14:50:24.428274Z | `ca36bc53c7b97f1b116f19c999adea22c1f5a8f0f6a4ef98c66554d3fb373ce0` |
| 5 | `eggserve-core` | 0.5.0 | 2026-10-22T14:51:08.782469Z | `c55ab91cc304ba45fc9d2d0a87fc136e2b0d795234d5c01b1ad059dde1851de9` |
| 6 | `eggserve-bin` | 0.2.3 | 2026-10-22T14:51:29.070245Z | `0cdd61605e470a16053834b25c2007fbe4d6f9b634d3adb6ed81f6de9eca3590` |

`eggnet-tls` was **not** republished: its source is unchanged, its published
`0.2.0` content still matches the local tree, and its `rustls` requirement
is unchanged by the advisory remediation. All six published versions are
`yanked=False`. No version was re-uploaded, altered, or yanked.

**Work package F — registry-only consumer proof.** Six fresh manifests in
`/tmp`, pinned to the exact published versions, no path dependencies,
resolved and built against the live registry only:

| Fixture | Resolved graph | Result |
|---|---|---|
| Direct H1 (default features) | `eggserve-server 0.5.0` → `eggserve-primitives 0.2.3` | **BUILD OK**; no `eggserve-core` / `eggserve-static` / `phf` on the graph |
| Direct H1 + Tower/Axum | `eggserve-server 0.5.0` → `eggserve-primitives 0.2.3`, `tower-service 0.3.3` | **BUILD OK**; direct graph still core/static/PHF-free |
| Direct H1 + TLS | `eggserve-server 0.5.0` → `eggserve-primitives 0.2.3` | **BUILD OK** |
| Core compatibility | `eggserve-core 0.5.0` → `eggserve-primitives 0.2.3`, `eggserve-server 0.5.0`, `eggserve-static 0.5.0` | **BUILD OK** |
| Core + H2 | `eggserve-core 0.5.0` (feature `http2`) | **BUILD OK** |
| Core + H3 | `eggserve-core 0.5.0` → `eggserve-h3 0.5.0`, `eggserve-primitives 0.2.3`; `quinn-proto 0.11.19` | **BUILD OK** |

The H3 consumer resolved `quinn-proto 0.11.19` — one patch past the
advisory-fixed `0.11.18` — confirming that downstream resolution picks up
the patched transitive versions through the existing caret ranges.

**Work package G — tag and GitHub Release.** Annotated tag `v0.5.0` at
`326aa1e714cdc2e19a0be8eda408d3d1af2c8d02`, pushed to `origin`. GitHub
Release "0.5.0 release line" (`v0.5.0`) published 2026-10-22T14:52:41Z, with
the per-crate checksum table, the corrective-line rationale, and the CI run
ID in the body.

**Work package H — wheel publication.** Release workflow run
**39799587527**, dispatched on `main` at the same SHA with
`publish_target=pypi`:

- `Preflight checks` — success (audits both lockfiles, release metadata,
  wheel-target authority, wheel-matrix validation).
- Eleven `Build wheel (…)` jobs — success (macOS x86_64 / arm64; Linux
  x86_64 glibc+musl; AArch64 glibc+musl; ARMv7 glibc+musl; Windows x86_64
  and ARM64).
- Five `ABI proof (CPython 3.11 … 3.15)` jobs — success.
- `Native AArch64 glibc`, `Native AArch64 musl (Alpine container)`,
  `Native Windows ARM64` — success.
- `Aggregate wheel set` — success (2026-10-22T15:03:47Z → 15:03:56Z).
- `Publish to TestPyPI` — **skipped** (the `pypi` target publishes to PyPI
  only).
- `Publish to PyPI` — **success** (2026-10-22T15:14:48Z → 15:15:04Z). The
  `pypi` environment requires a reviewer; the designated reviewer's own
  GitHub session approved the deployment, and the job then ran.
- Ten `Post-publish smoke: …` jobs — success.

**Propagation race, diagnosed and closed.** On the first attempt the
`Publish to PyPI` job **succeeded** but the post-publish smokes that started
22 seconds later failed with `Could not find a version that satisfies the
requirement eggserve==0.2.5 (from versions: 0.2.0, 0.2.4)` — the smokes
install from the public index, and PyPI's simple index had not yet listed
the new version. The wheel bytes were on PyPI throughout. The failed jobs
were re-run (`gh run rerun 39799587527 --failed`) once the index had
propagated; the run then completed with overall conclusion **success** and
all ten smokes green. An independent local check confirms it: a fresh venv
resolved `eggserve==0.2.5` from the index, reported
`eggserve.__version__ == "0.2.5"`, exposed the full `eggserve.server` /
`eggserve.lowlevel` surface, and ran the `eggserve` CLI.

Published wheel manifest (PyPI JSON API, `eggserve 0.2.5`,
`requires_python >=3.11`):

| Wheel | Uploaded (UTC) | SHA-256 | Size |
|---|---|---|---|
| `eggserve-0.2.5-cp311-abi3-macosx_11_0_arm64.whl` | 2026-10-22T15:14:48Z | `d1d9363ca832bdacb363c02a64df1c5aeaadb46fda02d423af4fdc3d8e1031e5` | 1327823 |
| `eggserve-0.2.5-cp311-abi3-macosx_11_0_x86_64.whl` | 2026-10-22T15:14:51Z | `1833b1389ab07d235f94233be9fe58a8859c9f16a87d2ca9797bfb08b8ff3904` | 1370730 |
| `eggserve-0.2.5-cp311-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl` | 2026-10-22T15:14:52Z | `35a7e4bd1292448fb5a77d1b066b43a6915737c78addcfee544656161e8b1bca` | 1461189 |
| `eggserve-0.2.5-cp311-abi3-manylinux_2_17_armv7l.manylinux2014_armv7l.whl` | 2026-10-22T15:14:54Z | `5d8a8e0b74d9df1f4e12c0c2b90c1b2b89693164dd3697a0c3dd8b478cc0b385` | 1199782 |
| `eggserve-0.2.5-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | 2026-10-22T15:14:55Z | `64356b68b87996a602cb33abc905415ce489fed89d33deb7c5fc4eb48bf1fd47` | 1461856 |
| `eggserve-0.2.5-cp311-abi3-musllinux_1_2_aarch64.whl` | 2026-10-22T15:14:57Z | `69d115185a970bd19fe84d90d310a035575f9144a9b7a29000049573174efd54` | 1641300 |
| `eggserve-0.2.5-cp311-abi3-musllinux_1_2_armv7l.whl` | 2026-10-22T15:14:59Z | `29b1ad7f5b760815565725128ef8da57fd5b15103f0c163f824fbbd0bc3cf696` | 1474295 |
| `eggserve-0.2.5-cp311-abi3-musllinux_1_2_x86_64.whl` | 2026-10-22T15:15:00Z | `bc8b4f2f05d76d8c8b1f6290109523fde96d3a1a21d4bb4ed8e489280810fecc` | 1682301 |
| `eggserve-0.2.5-cp311-abi3-win_amd64.whl` | 2026-10-22T15:15:02Z | `fca961f281e728312c291259db9593fbf3016483b2074733bb1b00917021fa13` | 1408166 |
| `eggserve-0.2.5-cp311-abi3-win_arm64.whl` | 2026-10-22T15:15:04Z | `a9a58533b9a87ed48015aedfd57cff08c168ffa783a4d9c4376636104edb4e95` | 1352599 |

PyPI's simple index lists all ten files with provenance attestations.

### 3.6 Work package I — changelog and migration guide

`CHANGELOG.md` now carries two new entries:

- **`## 0.5.0 — corrective line (publication pending)`** — the behavior
  changes, grouped by class, plus the three internal consolidations that
  have no observable change, plus the version table.
- **`## 0.4.0 — published 2026-09-25 (superseded by 0.5.0)`** — what was
  actually published on 2026-09-25, and the explicit statement that the
  published artifact **predates** the corrective batch, with the affected
  defect classes enumerated so a consumer of the live crates can judge
  their exposure.

`docs/migration-guide.md` gains a **"Plan 305: 0.5.0 corrective line"**
section: a per-area before/after table for every observable change, the
statement that `eggserve-core = "0.4"` consumers must widen to `"0.5"`,
and the pointer to the Plan 308 section for the source-level impact of the
four added `RuntimeConfig` fields.

### 3.4 What the corrective line carries besides the defect batch

The published line will also carry Milestones 306–308, which landed in the
same tree:

- **306** — the runtime error representation has one authority
  (`eggserve-primitives::canonical::runtime_error_with_policy`); the wire
  output is byte-identical.
- **307** — the Plan 253 connection-overlap ledger is corrected, measured,
  and machine-checked by a drift bound; the H3 `Alt-Svc` advertisement
  policy is single-sourced; six dead helpers are gone.
- **308** — the compatibility `RuntimeConfig` carries the four direct-H1
  ownership values with unchanged defaults, and the compatibility builder
  delegates 37 of its 38 shared setters to the direct builder. This is the
  one source-breaking element and it is why the line is `0.5.0`.

## 4. Verification executed

### 4.1 Commands run

```bash
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-core --features http2,http3 --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http3 --lib --tests -- -D warnings
cargo clippy -p eggserve-core --lib --tests -- -D warnings
cargo clippy -p eggserve-server --lib --tests -- -D warnings
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-primitives --lib
cargo test -p eggserve-h3 --lib
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2 --lib
cargo test -p eggserve-core --features http2,http3 --lib
cargo test -p eggserve-core --features http2,http3 --test alt_svc_advertisement
cargo test -p eggserve-core --test compatibility_config_projection
cargo test -p eggserve-core --features http2 --test runtime_error_single_authority_compat
cargo test -p eggserve-server --test runtime_error_single_authority
cargo test -p eggserve-static --lib
cargo check --workspace --all-targets
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo audit --file Cargo.lock
cargo audit --file crates/eggserve-python/Cargo.lock
cargo deny check
(cd crates/eggserve-python && cargo deny --config ../../deny.toml check)
```

### 4.2 Results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --features http2,http3 --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --features http3 --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-server --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings` | clean |
| `cargo test -p eggserve-primitives --lib` | ok — 312 passed |
| `cargo test -p eggserve-h3 --lib` | ok — 13 passed |
| `cargo test -p eggserve-server --lib` | ok — 89 passed, 1 ignored |
| `cargo test -p eggserve-core --lib` | ok — 158 passed |
| `cargo test -p eggserve-core --features http2 --lib` | ok — 171 passed |
| `cargo test -p eggserve-core --features http2,http3 --lib` | ok — 179 passed |
| `cargo test -p eggserve-core --features http2,http3 --test alt_svc_advertisement` | ok — 4 passed |
| `cargo test -p eggserve-core --test compatibility_config_projection` | ok — 10 passed |
| `cargo test -p eggserve-core --features http2 --test runtime_error_single_authority_compat` | ok — 2 passed |
| `cargo test -p eggserve-server --test runtime_error_single_authority` | ok — 2 passed |
| `cargo test -p eggserve-static --lib` | ok — 347 passed, 2 ignored |
| `cargo check --workspace --all-targets` | clean |
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | clean |
| `python3 scripts/check-crate-topology.py` | exit 0 |
| `python3 scripts/check-crate-topology.py --self-test` | 41 checks passed |
| `python3 scripts/verify-conformance-matrix.py` | 51 matrix entries; 55 app-server scenarios (47 routine); 17 Plan 213 HTTP/3 scenarios |
| `python3 scripts/check-python-release-metadata.py` | `Release preflight passed (version: 0.2.5)` |
| `cargo audit --file Cargo.lock` | exit 0 — no vulnerabilities (after §4.3) |
| `cargo audit --file crates/eggserve-python/Cargo.lock` | exit 0 — no vulnerabilities (after §4.3) |
| `cargo deny check` (root) | `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo deny --config ../../deny.toml check` (Python) | `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo test --workspace` (full) | **not run** — the targeted per-crate suites above cover every changed crate and every feature lane; the full workspace run is part of the publication sequence (Work package D's hosted CI). |
| `scripts/verify-cargo-packages.sh --mode all` | **not run** — a release-prep packaging gate that runs with the publish step. |

**Deviation from Plan 305 §7 B.** The acceptance criterion "`git diff
--stat` contains no `crates/**/src/**` entries" is satisfied *for the bump
surface* (manifests, lockfiles, and docs only — 16 files). The working
tree as a whole additionally carries Milestones 306–308's implementation
(14 source files, +2046/−696, plus 4 new test files), because those
milestones were implemented in the same tree before the bump. That is
intentional — the corrective line should carry them — but it means the
bump commit will **not** be a metadata-only diff. The maintainer must
commit the implementation and the version bump together, or split them
knowing that only the second is metadata-only.

### 4.3 The red gate: advisory remediation

`bash scripts/check-supply-chain.sh` initially **failed** with eight
advisories in three transitive dependencies:

| Crate | Resolved | Advisory | Date | Severity | Fix |
|---|---|---|---|---|---|
| `quinn-proto` | 0.11.17 | RUSTSEC-2026-0338 — remote peer can panic with zero-length STREAM frames during unordered reads | 2026-09-14 | — | ≥ 0.11.18 |
| `quinn-proto` | 0.11.17 | RUSTSEC-2026-0343 — outgoing datagram buffer accounting can panic the sender | 2026-09-14 | 7.5 (high) | ≥ 0.11.18 |
| `quinn-proto` | 0.11.17 | RUSTSEC-2026-0340 — peer minimum ACK delay can panic with ACK_FREQUENCY enabled | 2026-09-14 | 7.5 (high) | ≥ 0.11.18 |
| `rustls` | 0.23.45 | RUSTSEC-2026-0347 — buffered application data sent to the outer server after ECH rejection | 2026-10-14 | 2.3 (low) | ≥ 0.23.46 |
| `rustls` | 0.23.45 | RUSTSEC-2026-0349 — unauthenticated ServerHello can downgrade later connections to classical key exchange | 2026-10-14 | 3.7 (low) | ≥ 0.23.46 |
| `rustls` | 0.23.45 | RUSTSEC-2026-0348 — Brotli certificate decompression can allocate up to 1 GiB before authentication | 2026-10-14 | 3.7 (low) | ≥ 0.23.46 |
| `rustls` | 0.23.45 | RUSTSEC-2026-0346 — `Debug` impls for client session values expose TLS secrets | 2026-10-14 | 4 (medium) | ≥ 0.23.46 |
| `rustls-webpki` | 0.103.15 | RUSTSEC-2026-0382 — certificate verification may accept distrusted intermediates | 2026-10-20 | — | ≥ 0.103.16, < 0.104.0-alpha.1 |

**These advisories postdate the 0.4.0 publication** (2026-09-25), so the
live published crates carry them too; they are not a regression introduced
by this milestone.

**Remediation: lockfile-only.** `cargo update --precise` moved the three
resolved versions to `rustls 0.23.46`, `rustls-webpki 0.103.16`, and
`quinn-proto 0.11.18` in both lockfiles. **No manifest requirement was
changed.** That keeps four things true at once:

1. the `rustls` `0.23.45` caret floor stays in every constraining manifest
   (AGENTS.md: "never roll it back"), and the floor now *permits* the
   patched release;
2. `eggnet-tls`'s manifest is untouched, so its local content still matches
   the published `0.2.0` and it does **not** need republication — the
   known limitation from Plan 300 §5 is preserved rather than widened;
3. downstream consumers of the published crates resolve the patched
   versions automatically, because `^0.23.45` admits `0.23.46` and
   `^0.11.11` admits `0.11.18`;
4. the lockfile package set is unchanged — no dependency was added or
   removed, satisfying Plan 305 §6 ("requirement-bump-only").

After the remediation, both audits exit 0 and both policy checks report
`advisories ok, bans ok, licenses ok, sources ok`, and the workspace
rebuilds cleanly against the patched versions.

### 4.4 Local tool-version deviation (recorded, not a gate failure)

`scripts/check-supply-chain.sh` runs the Python-crate policy check as
`cargo deny check --config ../../deny.toml`. That argument order is
accepted by the repo's **pinned** `cargo-deny 0.19.0`
(`scripts/install-cargo-tools.sh`), but the locally installed `cargo-deny`
is **0.20.2**, which rejects `--config` after the subcommand and exits 1
with `unexpected argument '--config' found`. The equivalent 0.20.2 form,
`cargo deny --config ../../deny.toml check`, was used instead and reports
`advisories ok, bans ok, licenses ok, sources ok`.

This is an environment/version mismatch, not a policy failure and not a
defect introduced by this milestone. The script was **not** changed,
because its argument order is correct for the pinned tool version.

### 4.5 Published-state evidence (for the outstanding upload steps)

Live registry state, re-read on 2026-10-22, confirming that none of the
chosen versions already exists:

| Crate | Live version on crates.io | Version prepared here |
|---|---|---|
| `eggserve-primitives` | 0.2.2 | 0.2.3 |
| `eggserve-server` | 0.4.0 | 0.5.0 |
| `eggserve-static` | 0.4.0 | 0.5.0 |
| `eggserve-h3` | 0.4.0 | 0.5.0 |
| `eggserve-core` | 0.4.0 | 0.5.0 |
| `eggserve-bin` | 0.2.2 | 0.2.3 |
| `eggnet-tls` | 0.2.0 | unchanged (not republished) |
| `eggserve` (PyPI) | 0.2.4 | 0.2.5 |

## 5. Invariant review

| Invariant | Evidence | Status |
|---|---|---|
| Published crate versions are immutable; nothing published is re-uploaded, yanked, or altered | No publication was performed; the live state is unchanged (§4.5) | holds |
| The `rustls` `0.23.45` caret floor stays in every constraining manifest, including the excluded Python crate | `grep -rn rustls crates/*/Cargo.toml crates/eggserve-python/Cargo.toml` — every requirement is still `0.23.45` | holds |
| `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for both lockfiles | `cargo deny check` (root and Python) | holds |
| No new dependency enters the graph | The lockfile diff contains only version changes of existing packages | holds |
| Legacy flat plans and `release/` records stay immutable | None modified | holds |
| The manual publication workflow stays manual | No push, tag, merge, `cargo publish`, or workflow dispatch was executed | holds |
| Safe defaults unchanged (loopback bind, no symlinks, no dotfiles, no directory listing) | No policy default touched; the CLI grammar and the Python surface are untouched | holds |
| `eggserve-static` remains the sole static/path/filesystem authority | Not moved | holds |
| The `eggserve-primitives` crate stays transport-neutral | No new dependency; the new Alt-Svc helper takes a port, a flag, and a denylist slice | holds |
| Response framing belongs to the runtime only | Milestone 306 routes every runtime error through the authority's own normalization; framing is unchanged | holds |
| One-shot bodies / `OpsContext` logging / no library `println!` | No runtime change; clippy clean in every lane | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** The version line was chosen so
  that no chosen version exists on the live index (§4.5), so a retry
  cannot collide with an existing publication.
- **Cancellation races.** Not applicable.
- **Restart and shutdown/drain behavior.** Not applicable to the version
  bump. (Separately: B51 — shutdown drain was not bounded by
  `graceful_shutdown_timeout` — is a defect of the *published* 0.4.0
  artifact and is fixed in this batch.)
- **Partial failure and resource release.** The publication sequence is
  serial in dependency order with index-visibility confirmation between
  crates. Per Plan 305 §8, a partially published line is remediated with
  **new versions**, never by re-uploading. The rollback rule is therefore
  "cut a new patch", not "fix and re-push the same number".
- **Malformed or unauthorized input.** The crates.io token and the `pypi`
  environment approval are authorization inputs; a rejected upload must not
  be retried with the same version number.
- **Bounded event and artifact behavior.** The wheel set is bounded at ten
  targets by `release/wheel-matrix.toml`; the aggregate manifest job is
  the artifact the publish job consumes.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged. The four compatibility-config
  knobs added by Milestone 308 default to the direct crate's defaults.
- **Backward compatibility.** `0.4.0`-line consumers stay on the published
  artifacts; nothing is yanked. The corrective line is a **separate**
  version line, so `eggserve-core = "0.4"` does **not** resolve it — the
  migration guide states this explicitly.
- **Protocol negotiation.** Unchanged except for the documented
  fail-closed behavior of `version_to_http` / `from_state`.
- **Rollback limitations.** crates.io versions cannot be un-published;
  `eggserve-* 0.4.0` will remain resolvable forever. Mitigation is
  forward-only, which is why the changelog names the defect classes the
  live crates still carry.
- **Semver decision.** Recorded in §3.1 with a rationale per crate.
- **Registry-only consumer proof.** Outstanding — it can only run against
  the published versions (Work package F). The equivalent proof against
  the **published 0.4.0** artifacts is already recorded in
  `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` §3.3.
- **Legacy-path status.** The 0.3.x line and the `plans/280–292` /
  `release/plan-2xx` records remain valid history.

## 8. Security review

- **Confinement enforcement.** Unchanged by the version bump; the
  confinement-relevant corrections (B35 backslash inheritance, B36 platform
  component re-check, B39 error mapping, B48 buffer-export fail-closed,
  B58 listener pairing) are named in the changelog so operators can
  confirm the deployed version.
- **Safe-default preservation.** Unchanged.
- **Path validation.** Unchanged.
- **Privilege boundaries.** Unchanged.
- **Denial-of-service bounds.** Unchanged, except the documented B51
  shutdown-drain bound, which this line adds.
- **Redaction and audit.** Unchanged.
- **Dependency advisories.** The eight advisories in §4.3 were **red** at
  the start of the gate run and are **green** after the lockfile-only
  remediation. The published 0.4.0 artifacts still carry them; the
  corrective line, once published, does not. Two of the three
  `quinn-proto` advisories are high severity and affect the experimental
  H3 stack.

## 9. Documentation and operations

- **Updated:** `CHANGELOG.md` (0.5.0 and 0.4.0 entries),
  `docs/migration-guide.md` (Plan 305 section),
  `docs/release-process.md` (the new leaf line and the "published line is
  still 0.4.x until the publication is confirmed" statement), `README.md`
  (`eggserve-core = "0.5"`).
- **Updated:** all seven workspace manifests, the excluded Python crate's
  manifest, `pyproject.toml`, and both lockfiles.
- **Created:** `plans/closure/direct-h1-runtime/305-corrective-040x-publication.md`.
- **Operator diagnostics.** The changelog is the operator-facing record:
  it names the defect classes the live 0.4.0 crates carry and the version
  that fixes them.
- **Recovery instructions.** If an upload fails mid-sequence, do **not**
  re-upload the same version; cut a new patch (`0.5.1` / `0.2.4` /
  `0.2.6`) with the corrected content, per Plan 305 §8.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| resolved | The corrective line is **not published** | — | **Resolved** — published 2026-10-22 (§3.5). The live crates.io artifacts are now `primitives 0.2.3`, `server`/`static`/`h3`/`core 0.5.0`, `bin 0.2.3`. |
| resolved | The published 0.4.0 artifacts carry the eight advisories of §4.3 (three `quinn-proto`, four `rustls`, one `rustls-webpki`), two of them high severity | — | **Resolved** for new resolutions — the published `0.5.0` crates resolve the patched versions (`rustls 0.23.46`, `rustls-webpki 0.103.16`, `quinn-proto 0.11.18`+). The superseded `0.4.0` artifacts remain resolvable and still carry them; a consumer pinned to `=0.4.0` must move to the `0.5.0` line |
| medium | The manifest `rustls` floor was **not** raised to `0.23.46` | A consumer who pins `rustls = "=0.23.45"` explicitly still resolves the vulnerable release | A separate dependency-policy decision: raising the floor requires updating `AGENTS.md`'s stated `0.23.45` floor and every constraining manifest, and republishing `eggnet-tls` (whose content would then differ from the published `0.2.0`). Recommended as its own scoped plan |
| medium | The `pypi` TestPyPI staging step has never succeeded (the 0.4.0 release run failed on a TestPyPI trusted-publishing exchange failure; the corrective run used `publish_target=pypi`, which skips the TestPyPI job entirely) | No staging-package verification precedes the production PyPI upload | Configure the TestPyPI trusted publisher, then use `publish_target=testpypi` before `publish_target=pypi` on the next line |
| resolved | Feature-specific registry-only consumers had not been re-run against the corrective line | — | **Resolved** — six fixtures now run against the published line: direct, direct+Tower/Axum, direct+TLS, core, core+H2, core+H3 — all resolve and build (§3.5) |
| low | The locally installed `cargo-deny` is 0.20.2 while the repo pins 0.19.0, so `scripts/check-supply-chain.sh` fails at its Python policy step in this environment | The script's Python policy check could not be run verbatim here; the equivalent invocation was used and is green (§4.4). The hosted `supply-chain` CI job runs the pinned tools and was **success** on the published SHA | Install the pinned tools with `bash scripts/install-cargo-tools.sh` |
| resolved | `cargo test --workspace` and `scripts/verify-cargo-packages.sh --mode all` were not run at this stage | — | **Resolved** — the exact-SHA hosted CI run `39794834966` ran the full workspace suite (Clippy, workspace tests, MSRV lanes, Tower/interop qualification, the excluded Python crate, and the TLS/H2/H3 lint+test lanes) on the published commit, and the layered package dry-run passed locally |
| low | The published 0.4.0 crates expose 14 broken documentation links on crates.io (relative README links rewritten to `blob/HEAD/crates/<crate>/…`) | A reader of any published crate page cannot reach the security policy, CLI reference, migration guide, or threat model | **Fixed in this milestone** — the README's repository links are now absolute (§3.3a) |
| low | `eggnet-tls` local version `0.2.5` vs published `0.2.0` | Version-string drift; content identical | Resolve in a future manifest sweep |

No critical finding is open against the *process*. The two high findings are
the same outstanding publication action, which is why the milestone is
conditionally closed rather than closed.

## 11. Roadmap disposition

**Milestone closed.**

The semver decision is recorded with per-crate rationale; the version line
is applied to every manifest and both lockfiles; the changelog and the
migration guide describe the artifact consumers can reach; the crates.io
metadata that consumers actually see was audited and repaired; the local
gate list is green after the advisory remediation of §4.3; and the line is
published with an exact-SHA CI proof, per-crate checksums, six
registry-only consumer proofs, a tag, and a GitHub Release (§3.5).

The two residual items — the unconfigured TestPyPI trusted publisher and the
decision to raise the manifest `rustls` floor — are recorded in §10 with
their own required actions. Neither blocks this milestone.

## 12. Registry updates

`plans/registry.md`:

- The Direct H1 runtime row for Milestone 305 moves `ready` →
  `conditionally closed`, with this closure record linked and the
  outstanding publication steps named.
- The "Blocked work" section now names the corrective republication as
  *prepared but not published*, with the reason.

`plans/subsystems/direct-h1-runtime-roadmap.md`:

- Milestone 11 (Plan 305) moves `ready` → `conditionally closed` in §12,
  with this closure record and the outstanding steps named.
- §4 "Current state" records that the corrective line is prepared and
  gated, and that the upload, tag, release, and wheel steps remain
  outstanding.
