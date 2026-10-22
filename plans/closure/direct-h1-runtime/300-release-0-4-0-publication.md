# Direct H1 Runtime Milestone 300 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/300-release-0-4-0-publication.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-9--plan-300--release-the-040-line`

Corrective milestone that closes this one:

- `plans/implementation/direct-h1-runtime/304-040-publication-closure.md`
- closure: `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md` (this record)

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`
(`Plan 303: rewrite README quickstart, fix user-docs accuracy (docs-only)`)

Publication artifacts reviewed (immutable, already public):

- crates.io — `eggserve-primitives 0.2.2`, `eggserve-server 0.4.0`,
  `eggserve-static 0.4.0`, `eggserve-h3 0.4.0`, `eggserve-core 0.4.0`,
  `eggserve-bin 0.2.2`, `eggnet-tls 0.2.0`
- PyPI — `eggserve 0.2.4`
- Git — tag `v0.4.0` → `d537d4dd7e0102c9ffe6101caf7dc2fad8b7f494`
- GitHub — Release "0.4.0 release line" (`v0.4.0`), published
  2026-09-25T19:28:57Z

Implementation commits or pull requests:

- `d537d4d` — `Plan 300: bump to the 0.4.0 release line + wheel 0.2.4`
  (the published commit; tag `v0.4.0`)
- crates.io publish — 2026-09-25T19:22:37Z … 19:25:06Z (seven crates,
  sequential)
- PyPI publish — 2026-09-25T20:25:35Z … 20:26:16Z (ten wheels)
- `36176532051` — CI run on `d537d4d` (`push`), conclusion `success`
- `36181783819` — Release workflow run on `d537d4d`
  (`workflow_dispatch`), conclusion `success`

## 1. Executive finding

Plan 300's publication **did happen** and is fully evidenced: the 0.4.0
line is on crates.io, the `eggserve 0.2.4` wheel set is on PyPI, the
`v0.4.0` tag and the GitHub Release exist, the exact-SHA hosted CI run
passed, and registry-only consumers of the published artifacts resolve and
build.

The milestone **cannot close cleanly**, because the published artifact does
not contain the code that shipped in the repository. The `v0.4.0` tag
points at `d537d4d`, and **eight commits landed after that tag**, five of
which change production code. The published `eggserve-* 0.4.0` crates
therefore do not contain any of B1–B89, nor the CI fix, nor the
docs-only Plans 301–303.

This record therefore closes Plan 300 **conditionally**: the publication
is real and verified; the open condition is that the published artifact
predates the post-tag corrective batch and must be superseded by a
corrective republication. That republication is Milestone 305
(`plans/implementation/direct-h1-runtime/305-corrective-040x-publication.md`).

Consumers of the published 0.4.0 crates are exposed to the defect classes
enumerated in §3.3 until the corrective line supersedes them.

## 2. Requirement-to-evidence matrix

Plan 300 §13 acceptance criteria, mapped to collected evidence.

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| 0.4.0 line published to crates.io (six workspace crates) | crates.io API, per-crate checksum + timestamp (§3.1) | pass | All six present, none yanked. |
| `eggnet-tls` published | crates.io API, `eggnet-tls 0.2.0` (§3.1) | pass | Published 2026-09-22, before the 0.4.0 line; see §3.5. |
| Wheel published to PyPI | PyPI JSON API, ten wheels (§3.4) | pass | `eggserve 0.2.4`, `requires_python >=3.11`. |
| TestPyPI → PyPI sequence | Release run `36179961100` (TestPyPI publish `failure`) and `36181783819` (TestPyPI `skipped`, PyPI publish `success`) (§3.4) | partial | TestPyPI was **never** published: the first run failed with a trusted-publishing configuration error and the second skipped the step. PyPI publication succeeded. See §10. |
| `v0.4.0` tag exists and points at the published commit | `git rev-list -n1 v0.4.0` → `d537d4dd…` | pass | Commit subject: `Plan 300: bump to the 0.4.0 release line + wheel 0.2.4`. |
| GitHub Release exists for the line | Release "0.4.0 release line", `v0.4.0`, published 2026-09-25T19:28:57Z | pass | `targetCommitish: main`. |
| Exact-SHA hosted CI green for the bump commit | CI run `36176532051`, `headSha = d537d4dd…` (§3.2) | pass | `rust`, `supply-chain`, `python` all `success`. |
| Manifest versions match Plan 300 §6 | Local manifests vs the published version set | pass | `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`. |
| `eggserve-core 0.4.0` resolves the Plan 300 §6 requirement set | crates.io dependency resolution (§3.3) | pass | `eggserve-core 0.4.0` → `eggserve-primitives 0.2.2`, `eggserve-server 0.4.0`, `eggserve-static 0.4.0`; `eggserve-h3`/`eggnet-tls` are optional-feature dependencies. |
| Direct-graph purity: no `eggserve-core`/`eggserve-static`/`phf` on the direct graph | Registry-only consumer, `cargo tree -e no-dev` (§3.3) | pass | Direct consumer resolves only `eggserve-server 0.4.0` → `eggserve-primitives 0.2.2`. |
| Registry-only consumers build against the published artifacts | Two fresh `/tmp` manifests, locked, `cargo check` (§3.3) | pass | Direct H1 consumer: `BUILD OK`. Core compatibility consumer: `BUILD OK`. |
| Direct H1 **with TLS**, Tower/Axum, and core-with-H2/H3 registry consumers | — | not run | The `tls` / `http2` / `http3` feature edges pull optional crates; a full matrix would need feature-specific consumer manifests. The two consumers built cover the default feature sets of the two primary graphs. Recorded as outstanding in §10. |
| Post-publish wheel smoke on the published wheel set | Release run `36181783819`, ten `Post-publish smoke: …` jobs | pass | All ten `success` (2026-09-25T20:26:19Z … 20:28:40Z). |
| Aggregate wheel manifest published | Release run `36181783819`, `Aggregate wheel set: success`; PyPI file list (§3.4) | pass | Ten wheels, ten distinct SHA-256 digests. |
| Published crate versions are immutable | No re-upload, no yank, no version change | pass | All seven crates `yanked=False`. |
| `RUSTSEC-2026-0285` `rustls 0.23.45` caret floor retained in every constraining manifest | `deny.toml` + both lockfiles (§4) | pass | `scripts/check-supply-chain.sh` audited both lockfiles; no wildcard or `native-tls`/`openssl-sys` admission. |
| `CHANGELOG.md` records the published line | `CHANGELOG.md` has no `0.4.0` heading; newest is `## 0.3.1` | fail | Deliberately deferred to Milestone 305: the changelog entry must describe the artifact consumers can actually reach, which is the corrective line, not the pre-fix 0.4.0 line. |
| Planning control surface reports Plan 300 correctly | Pre-milestone state: `plans/registry.md` Plan 300 = `active`; no closure record existed | fail (at baseline) | Corrected by this milestone — see §12. |

## 3. Production implementation evidence

### 3.1 Published crates — checksum and timestamp table

Collected from the live crates.io API on 2026-10-22.

| Crate | Version | Published (UTC) | SHA-256 checksum | Yanked |
|---|---|---|---|---|
| `eggserve-primitives` | 0.2.2 | 2026-09-25T19:22:37.877495Z | `78fd797e45a374bfa419bc75f7e497653ce25e24cde0e771cc332e267f18355c` | no |
| `eggserve-server` | 0.4.0 | 2026-09-25T19:23:22.001606Z | `fb601019a2914ae99f264640b66c80496a67b7ea3315fd0809747d4f22327e40` | no |
| `eggserve-static` | 0.4.0 | 2026-09-25T19:23:55.116837Z | `c53bf2f2d92e660940676eb8b6bdff2ceae5cc4aedaab1f22e7dccaa8a056f14` | no |
| `eggserve-h3` | 0.4.0 | 2026-09-25T19:24:26.520798Z | `ee9b47c7ca050cea49c40eb1fe6489948948b6d666fc3b2386983ef333c55f8a` | no |
| `eggserve-core` | 0.4.0 | 2026-09-25T19:24:49.080894Z | `5cf75d63bdfbfa70a92c7884220addf032382099d3adabbd1bd38c0e798e48a7` | no |
| `eggserve-bin` | 0.2.2 | 2026-09-25T19:25:06.587936Z | `c2e325351425331e9d6b330b0edb1c73ef4a0fccba3e4a85df617dabb3db4e45` | no |
| `eggnet-tls` | 0.2.0 | 2026-09-22T21:37:32.208651Z | `12668788d6f8bbc2cd1f64b936563d08f137b5966e50e5688ec5fdd4cf576d55` | no |

Publication was sequential over roughly three minutes, in dependency
order (primitives → server → static → h3 → core → bin), which is
consistent with a deliberate publish sequence rather than a bulk upload.

### 3.2 Hosted CI on the published commit

CI run `36176532051`, `headSha = d537d4dd7e0102c9ffe6101caf7dc2fad8b7f494`,
event `push`, overall conclusion `success`.

| Job | Conclusion | Started (UTC) | Completed (UTC) |
|---|---|---|---|
| `python` | success | 2026-09-25T18:57:44Z | 2026-09-25T19:00:58Z |
| `supply-chain` | success | 2026-09-25T18:58:25Z | 2026-09-25T19:04:55Z |
| `rust` | success | 2026-09-25T18:58:55Z | 2026-09-25T19:18:10Z |

Advisory-scan runs on the same SHA: `36565270375` (success),
`36423727718` (success), `36315172021` (success), `36236591617`
(success) — 2026-09-26 → 2026-09-29, all scheduled.

### 3.3 Registry-only consumer proofs

Fresh manifests in `/tmp`, pinned to the exact published versions, no
path dependencies, resolved and built against the live registry only.

**Direct H1 consumer** — `eggserve-server = "=0.4.0"`:

```
$ cargo tree -e no-dev --depth 3
└── eggserve-server v0.4.0
    ├── eggserve-primitives v0.2.2

$ cargo tree -e no-dev --prefix none | grep -E '^(eggserve-core|eggserve-static|phf)'
(no matches)
OK: no eggserve-core / eggserve-static / phf on the direct graph

$ cargo check
direct consumer: BUILD OK
```

**Core compatibility consumer** — `eggserve-core = "=0.4.0"`:

```
$ cargo tree -e no-dev --depth 2
└── eggserve-core v0.4.0
    ├── eggserve-primitives v0.2.2
    ├── eggserve-server v0.4.0
    ├── eggserve-static v0.4.0

$ cargo check
core consumer: BUILD OK
```

`eggserve-h3 0.4.0` and `eggnet-tls 0.2.0` are optional-feature
dependencies of `eggserve-core` and therefore do not appear in the
default-feature tree.

### 3.4 Wheel publication

Release workflow run `36181783819` on `d537d4d`, event
`workflow_dispatch`, overall conclusion `success`.

- `Preflight checks: success`
- Eleven `Build wheel (…)` jobs: success
- `Native AArch64 musl (Alpine container)`, `Native Windows ARM64`,
  `Native AArch64 glibc (CPython 3.11)`: success
- Five `ABI proof (CPython 3.11 … 3.15)` jobs: success
- `Aggregate wheel set: success` (2026-09-25T20:01:49Z → 20:02:00Z)
- `Publish to TestPyPI: skipped`
- `Publish to PyPI: success` (2026-09-25T20:25:35Z → 20:26:16Z)
- Ten `Post-publish smoke: …` jobs: success (2026-09-25T20:26:19Z →
  20:28:40Z)

The earlier Release run `36179961100` (same SHA, started
2026-09-25T19:29:09Z) built the same wheel set successfully but failed at
`Publish to TestPyPI` with a PyPI trusted-publishing exchange failure
("Trusted publishing exchange failure … Token request failed"), leaving
`Publish to PyPI` skipped. The rerun skipped TestPyPI and published to
PyPI directly. The PyPI wheel set is therefore complete and verified; the
TestPyPI staging step never ran.

Published wheel manifest (PyPI JSON API, `eggserve 0.2.4`,
`requires_python >=3.11`):

| Wheel | Uploaded (UTC) | SHA-256 | Size |
|---|---|---|---|
| `eggserve-0.2.4-cp311-abi3-macosx_11_0_arm64.whl` | 2026-09-25T20:25:55Z | `0ca17c61a9e992b291cd65d1ba2722402bced9f22d0339f689b8d002bd6cf568` | 1319814 |
| `eggserve-0.2.4-cp311-abi3-macosx_11_0_x86_64.whl` | 2026-09-25T20:25:57Z | `70886389f123cb8cf3b6a01a4daf4d9043a186ac271ada55c9c7f7f1ace43a6c` | 1365121 |
| `eggserve-0.2.4-cp311-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl` | 2026-09-25T20:25:59Z | `1dc706abbce47abc67b99c1fb94fe0206caba8ab87e562de0c92a042e1e72128` | 1454848 |
| `eggserve-0.2.4-cp311-abi3-manylinux_2_17_armv7l.manylinux2014_armv7l.whl` | 2026-09-25T20:26:01Z | `37765eb7a3b81569d86fff2a3a108321749af4226be7dcf053921da39ea2e445` | 1194340 |
| `eggserve-0.2.4-cp311-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | 2026-09-25T20:26:03Z | `52208a9cc11618c6301723cc2345e64b95cc2eaeca051fdb3c43348c4faf04e0` | 1457710 |
| `eggserve-0.2.4-cp311-abi3-musllinux_1_2_aarch64.whl` | 2026-09-25T20:26:05Z | `8bac1e9296e3e0137a74275e89783d19e94b564961b2f3fb40e377912183384a` | 1632870 |
| `eggserve-0.2.4-cp311-abi3-musllinux_1_2_armv7l.whl` | 2026-09-25T20:26:06Z | `b915106f9d2679f29fc121d13ec4f2e6e6b6f114013fe920dca04d774cc90545` | 1469246 |
| `eggserve-0.2.4-cp311-abi3-musllinux_1_2_x86_64.whl` | 2026-09-25T20:26:08Z | `9925431b78f1f94e01a41dd2c36e6116363479f6cf48fc21259c3d822db61a9d` | 1674869 |
| `eggserve-0.2.4-cp311-abi3-win_amd64.whl` | 2026-09-25T20:26:10Z | `8fc20aa212f91c06d390d23f38bf1b2fd9640df76af7154c73f6f62a7afc149f` | 1400634 |
| `eggserve-0.2.4-cp311-abi3-win_arm64.whl` | 2026-09-25T20:26:12Z | `029c61008ba16e534adefc647df69e228fde1a5fd3aecf6838804f8d89630f5b` | 1348344 |

### 3.5 The condition — the published artifact predates the post-tag batch

`git rev-list v0.4.0..HEAD` yields **eight** commits:

| Commit | Subject | Production scope |
|---|---|---|
| `465b80c` | `Plan 303: rewrite README quickstart, fix user-docs accuracy (docs-only)` | none (docs) |
| `778b1db` | `Plan 302: sweep skill/agent-guide/docs/architecture version drift onto 0.4.0 line (docs-only)` | none (docs) |
| `6ddd316` | `Plan 301: refresh architecture overview + deep dives to 0.4.0 baseline (docs-only)` | none (docs) |
| `715281b` | `Fix CI: restore queued Python callback permits + collision-proof test temp files` | `eggserve-python`, `eggserve-server` |
| `ef51fa8` | `Fix B44-B89 bugs from bugs.md interrogation (minimal, no scope creep)` | `eggserve-server`, `eggserve-core`, `eggserve-static`, `eggserve-python` |
| `1f10632` | `Fix B10-B43 bugs from bugs.md interrogation (except evaluated non-issues)` | `eggserve-server`, `eggserve-core`, `eggserve-static`, `eggserve-python` |
| `c00e1b4` | `Fix B1-B9 bugs from bugs.md interrogation` | `eggserve-python`, `eggserve-server`, `eggserve-static` |
| `cb68c02` | `Fix bugs.md findings: IoError 500, conditional combine, try_into_duplex, trailer mapping` | `eggserve-h3`, `eggserve-python`, `eggserve-server`, `eggserve-static` |

`git diff --shortstat v0.4.0..HEAD -- crates/` → **53 files changed, 1646
insertions(+), 522 deletions(-)**. Whole tree: 89 files changed, 2596
insertions(+), 659 deletions(-).

None of B1–B89 and none of the CI fix is present in the published
artifacts. Representative user-visible and security-relevant classes, as
classified by the fixing commits:

- **Conditional-request correctness** — B1 (`If-Match *` succeeds without an
  ETag, RFC 9110 §13.1.1); B14 (unsatisfiable `Range` + failed `If-Range`
  serves 200 full instead of the RFC 9110 §13.1.5 behavior); B18 (header
  combining used last-wins for `Range`/`If-Range`/dates instead of
  first-wins; comma-join is correct only for `If-Match`/`If-None-Match`).
- **Protocol strictness** — B21 (`Http1Upgrade` accepted a bare `101`
  without a protocol); B39 (`PrematureEof`/`LengthMismatch` did not map to
  `IncompleteError`); B64/B71 (`version_to_http` / `from_state` could panic
  or silently downgrade instead of failing closed).
- **Authority / host validation** — B31 (the Tower adapter did not require
  the authority to agree with a mutated `Host`).
- **Confinement / path resolution** — B35 (child resolution did not inherit
  the confined backslash policy); B36 (fd-relative resolution did not
  re-apply platform component checks).
- **Resource and stream handling** — B48 (a buffer-export failure truncated
  the stream silently); B51 (shutdown drain was not bounded by
  `graceful_shutdown_timeout`); B58 (a listener/address pair could be
  mispaired and panic).
- **Python facade** — the CI fix restored queued Python callback permits
  and made test temp files collision-proof.

### 3.6 `eggnet-tls` version observation (known limitation)

The `eggnet-tls` workspace manifest version is `0.2.4`; the published
crate is `0.2.0`. Plan 300 §5 explicitly excluded an `eggnet-tls`
publication and recorded the drift as pre-existing. `eggnet-tls` source
has not changed since 2026-09-18, i.e. before the `0.2.0` publication on
2026-09-22, so the published content matches the local tree. This is a
version-string drift, not a content drift. Carried forward unchanged.

## 4. Verification executed

### Commands run

```bash
# live registry state
curl -sL -A "eggserve-publication-audit/1.0" "https://crates.io/api/v1/crates/<crate>"   # 7 crates
curl -sL "https://pypi.org/pypi/eggserve/json"

# git facts
git rev-list -n1 v0.4.0
git log --oneline v0.4.0..HEAD
git diff --shortstat v0.4.0..HEAD -- crates/
git diff --shortstat v0.4.0..HEAD

# hosted CI + release
gh run view 36176532051 --json jobs
gh run view 36181783819 --json jobs
gh run view 36179961100 --log-failed
gh release view v0.4.0 --json tagName,name,publishedAt,url

# registry-only consumer proofs
cargo generate-lockfile && cargo tree -e no-dev --depth 3   # direct H1 consumer
cargo generate-lockfile && cargo tree -e no-dev --depth 2   # core compatibility consumer
cargo check                                                 # both consumers
```

### Results

| Check | Result |
|---|---|
| crates.io API reachability + payload | pass (HTTP 200, JSON parsed for all seven crates) |
| PyPI JSON API reachability + payload | pass (HTTP 200, ten 0.2.4 wheels enumerated) |
| `git rev-list -n1 v0.4.0` | pass → `d537d4dd7e0102c9ffe6101caf7dc2fad8b7f494` |
| Post-tag commit enumeration | pass → 8 commits, 5 production-changing |
| `git diff --shortstat v0.4.0..HEAD -- crates/` | pass → 53 files, +1646 / −522 |
| CI run `36176532051` job conclusions | pass → `rust`, `supply-chain`, `python` all success |
| Release run `36181783819` job conclusions | pass → PyPI publish success; 10 post-publish smokes success |
| Release run `36179961100` failure diagnosis | pass (failure diagnosed, not hidden): TestPyPI trusted-publishing exchange failure |
| GitHub Release metadata | pass → `v0.4.0`, 2026-09-25T19:28:57Z |
| Direct H1 registry consumer: resolve + build | pass |
| Direct H1 registry consumer: ancestry purity | pass → no `eggserve-core`/`eggserve-static`/`phf` |
| Core registry consumer: resolve + build | pass |
| TLS / Tower-Axum / core-H2 / core-H3 registry consumers | **not run** — see §2 and §10 |
| Repository test suite re-run at this baseline | **not run** — not required by Plan 300 §13 (the published SHA's own CI run is the evidence) and out of this milestone's scope |

No secret material was read, printed, or recorded. The `gh` CLI's
credential store was used for read-only API access; no token value was
emitted into this record.

## 5. Invariant review

| Invariant (Plan 300 §14 / Plan 304 §4) | Evidence | Status |
|---|---|---|
| Published crate versions are immutable | All seven crates `yanked=False`; no version was re-uploaded, yanked, or altered by this milestone | holds |
| `rustls` `0.23.45` caret floor (`RUSTSEC-2026-0285`) retained in every constraining manifest, including the excluded Python crate | `deny.toml` + both lockfiles audited by `scripts/check-supply-chain.sh`; the published `eggnet-tls 0.2.0` predates the floor check but its content is unchanged since 2026-09-18 | holds |
| `deny.toml` wildcard and `native-tls`/`openssl-sys` bans stay green for both lockfiles | Supply-chain CI job on the published SHA: success | holds |
| Legacy flat plans and `release/` records stay immutable | No file under `plans/0*.md`–`plans/2*.md` or `release/` was modified by this milestone | holds |
| A closure record may not imply the published artifact contains the post-tag fixes | §3.5 states the divergence explicitly; §1 and §11 repeat it | holds |
| No runtime, API, dependency, or tier change lands in this milestone | This milestone produced planning artifacts only; `git diff --stat` over `crates/` is empty | holds |
| The manual publication workflow stays manual | No push, tag, merge, `cargo publish`, or `twine` was executed by this milestone; all publication evidence is read-only observation | holds |
| Safe defaults unchanged (loopback bind, no symlinks, no dotfiles, no directory listing) | No production change; the published artifact's defaults are the ones in effect at `d537d4d` | holds for the published artifact; Milestone 305 is the surface that carries the post-tag corrections |
| `eggserve-static` remains the sole static/path/filesystem authority | No ownership change; registry-only consumer ancestry confirms `eggserve-static` is not on the direct graph | holds |
| Static serving remains the primary product; CLI remains static-only | No CLI or policy change | holds |

## 6. Failure and recovery review

- **Duplicate delivery / idempotency.** The Release workflow was dispatched
  twice against the same SHA. The first dispatch published nothing to PyPI
  (it failed at TestPyPI, before the PyPI step) and the second published
  the wheel set once. PyPI holds exactly one `eggserve 0.2.4` file set —
  no duplicate upload exists, and the ten wheels have ten distinct
  digests. crates.io holds exactly one version per published crate.
- **Cancellation races.** Not applicable — publication is a one-shot
  upload, not a long-lived server.
- **Restart / shutdown / drain.** Not applicable to the publication
  itself. (Separately: B51 — shutdown drain was not bounded by
  `graceful_shutdown_timeout` — is a defect of the *published* artifact
  and is fixed in the post-tag batch. See §3.5.)
- **Partial failure and resource release.** The first Release run is the
  partial-failure case: eleven wheel builds succeeded, the aggregate
  manifest succeeded, and the failure was isolated to the TestPyPI
  publication step. The PyPI publication was skipped by the workflow's
  dependency graph rather than proceeding with a half-published state.
  Recovery was a re-dispatch with the TestPyPI step skipped.
- **Malformed or unauthorized input.** Not applicable.
- **Bounded event and artifact behavior.** The wheel set is bounded at ten
  wheels across eleven target configurations; the aggregate manifest job
  succeeded and is the artifact the PyPI job consumed.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged by this milestone.
- **Backward compatibility.** The published 0.4.0 line is additive over
  the 0.3.x line for consumers who pin `^0.4.0`. The corrective line
  (Milestone 305) will be a semver-pinned successor; consumers who pin
  the exact `=0.4.0` versions remain on the defective artifact until they
  move.
- **Protocol negotiation.** Unchanged.
- **Rollback limitations.** crates.io versions cannot be un-published;
  `eggserve-* 0.4.0` will remain resolvable forever. Mitigation is
  therefore *forward-only*: publish a corrected successor line and
  document the defect classes in the changelog. Yanking was considered and
  rejected — it would break existing consumers of a public API and is
  disproportionate to defect classes that are correctness fixes rather
  than unsoundness.
- **Semver decision (restated from Plan 300 §6).** The published line is
  `eggserve-primitives 0.2.2`, `eggserve-server`/`eggserve-static`/
  `eggserve-h3`/`eggserve-core 0.4.0`, `eggserve-bin 0.2.2`,
  `eggserve` (wheel) `0.2.4`, `eggnet-tls 0.2.0`. `server`/`static`/`h3`/
  `core` share a version because they are published as one line.
- **Registry-only consumer proof.** Provided in §3.3 (two consumers, both
  build). Feature-specific consumers remain outstanding — §10.
- **Legacy-path status.** The 0.3.x line and the `plans/280–292` /
  `release/plan-2xx` records remain valid history and are unaffected.

## 8. Security review

Confinement enforcement, path validation, privilege boundaries, and
redaction are **unchanged** by this milestone — no production code was
touched. The published artifact's security posture is the one in effect at
`d537d4d`.

Security-relevant defect classes present in the published artifact
(because their fixes are in the post-tag batch):

| Class | Identifiers | Exposure |
|---|---|---|
| Authority/host validation | B31 | A Tower-adapter consumer could be served by a mutated `Host` that the authority did not agree with — request-smuggling-adjacent, directly relevant to any deployment behind a proxy that rewrites `Host`. |
| Confinement inheritance | B35, B36 | Child-path resolution did not inherit the confined backslash policy, and fd-relative resolution did not re-apply platform component checks — both are path-confinement boundary weaknesses. |
| Silent stream truncation | B48 | A buffer-export failure truncated the stream silently, so a client could receive a short body with a success status and no error signal. |
| Listener/address pairing | B58 | A listener/address pair could be mispaired and panic — availability impact on a shared runtime. |
| Fail-open protocol downgrade | B64, B71 | `version_to_http` / `from_state` could panic or silently downgrade instead of failing closed — protocol-negotiation integrity. |
| Bounded shutdown | B51 | Shutdown drain was not bounded by `graceful_shutdown_timeout` — availability impact on shutdown. |

These are recorded so that consumers of the published `0.4.0` crates can
judge their exposure before Milestone 305 supersedes them. The corrective
line is the mitigation; nothing in this milestone alters the published
bytes.

## 9. Documentation and operations

- **Created:** `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
  (this record).
- **Updated:** `plans/registry.md` — Plan 300 row, "Blocked work" section,
  "Closure work and current control points" table, next-number line.
- **Updated:** `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 9
  status and blockers, §4 "Current state", Milestone 10 row.
- **Not written here, deliberately:** the `CHANGELOG.md` `0.4.0` entry.
  Plan 304 §6 and Plan 305 §5 both assign that entry to Milestone 305,
  because the changelog must describe the artifact users can actually
  consume — the corrective line — rather than the pre-fix 0.4.0 line this
  record describes.
- **Not written here, deliberately:** the doc/skill/architecture wording
  sweep that still describes the 0.4.0 line as pending
  (`.opencode/skills/eggserve-dev/SKILL.md:163-169`,
  `architecture/crate-topology.md:149`, `architecture/overview.md:65`,
  `architecture/eggserve-core.md:19`, `architecture/eggserve-bin.md:164`).
  That is Plan 314's scope.
- **Operator diagnostics:** none added; this milestone changes no runtime.
- **Static guards:** none added; the topology gate is unaffected by a
  publication record.
- **Recovery instructions:** if a corrective republication is needed, the
  input range is `v0.4.0..465b80c` plus whatever lands before the bump
  commit; the procedure is Milestone 305's.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | The published `0.4.0` crates do not contain B1–B89, the CI fix, or Plans 301–303 (post-tag range `v0.4.0..465b80c`, 53 files under `crates/`, +1646/−522) | Consumers of the published crates run with the defect classes enumerated in §3.5, including authority-validation, confinement-inheritance, and fail-open-negotiation weaknesses | Publish a corrective line — Milestone 305, `plans/implementation/direct-h1-runtime/305-corrective-040x-publication.md` |
| medium | TestPyPI publication never ran: run `36179961100` failed with a PyPI trusted-publishing exchange failure and run `36181783819` skipped the step | No staging-package verification preceded the production PyPI upload; the release path's TestPyPI stage is currently unproven | Configure the TestPyPI trusted publisher, then re-verify the TestPyPI → PyPI sequence on the corrective line (Milestone 305) |
| medium | `CHANGELOG.md` has no entry for the published 0.4.0 line | Consumers have no in-repo record of what the published artifact contains or does not contain | Milestone 305 writes the changelog entry for the corrective line |
| medium | Feature-specific registry-only consumers (direct H1 + TLS, Tower/Axum, core + H2, core + H3) were not run | The registry-only proof covers the default feature sets of the two primary graphs; optional-feature edges are unproven against the published artifacts | Re-run the four-consumer matrix against the corrective line (Milestone 305) |
| low | `eggnet-tls` workspace version is `0.2.4` while the published crate is `0.2.0` | Version-string drift; content is identical (unchanged since 2026-09-18) | Resolve in the corrective line's manifest sweep (Milestone 305) |
| low | Six documentation/skill surfaces still describe the 0.4.0 line as pending | Agent-facing and architecture guidance is stale | Plan 314 |

No critical finding is open against the *process* — the publication itself
was executed correctly and is fully evidenced. The high finding is a
known, bounded, and scheduled correction, which is why the milestone is
conditionally closed rather than blocked.

## 11. Roadmap disposition

**Milestone conditionally closed with a named corrective publication
outstanding.**

- Plan 300's publication capability is complete and evidenced: the 0.4.0
  line is on crates.io, the wheel set is on PyPI, the tag and GitHub
  Release exist, the exact-SHA CI run is green, and registry-only
  consumers build.
- The open condition is precise and bounded: the published artifact
  predates `v0.4.0..465b80c`. Milestone 305 owns the corrective
  republication.
- The next dependency may proceed: Milestone 305 may be dispatched, and
  Milestones 306–309 (code-only, no publication) are unaffected by this
  condition.

## 12. Registry updates

`plans/registry.md`:

1. Plan 300 row: `active` → `conditionally closed`, with the closure
   record link `plans/closure/direct-h1-runtime/300-release-0-4-0-publication.md`
   and the open condition named ("published artifact predates
   `v0.4.0..465b80c`").
2. "Blocked work": the "Plan 300 is active (operational needs: crates.io
   token present, `pypi` environment approval)" sentence is replaced by
   the truthfully blocked item — the corrective republication under
   Milestone 305.
3. "Closure work and current control points": a Plan 300 closure row is
   added.
4. Rows for Milestones 304–315 are added (done when the corrective program
   was registered).
5. The next-number line is `316`.

`plans/subsystems/direct-h1-runtime-roadmap.md`:

1. Milestone 9 (Plan 300): `active` → `conditionally closed`; the blockers
   field is updated to the corrective republication.
2. Milestone 10 (Plan 304) row added, with this closure record.
3. §4 "Current state" rewritten so it no longer describes Milestone 9 as
   the active work; it records that the 0.4.0 line is published, that the
   published artifact predates the post-tag batch, and that the
   corrective republication is outstanding.
