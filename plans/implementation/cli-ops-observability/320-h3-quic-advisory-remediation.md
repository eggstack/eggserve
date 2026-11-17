# Plan 320 — QUIC advisory remediation and supply-chain gate restoration

## 1. Status

- **State**: ready for handoff
- **Plan number**: 320
- **Subsystem**: CLI, ops, and observability
  (`plans/subsystems/cli-ops-observability-roadmap.md`, Milestone 320)
- **Source roadmap**: `plans/subsystems/cli-ops-observability-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (safe defaults; dependency and
    verification policy)
  - `plans/001-terminology-and-domain-model.md` (`RuntimeConfig`,
    verification vocabulary)
  - `plans/003-planning-process.md` §6 and §10 (required verification and
    planning review)
- **Primary class**: invariant (supply-chain security gate) + infrastructure
  (resolved-version floors and their guard)

## 2. Repository baseline

Repository baseline: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e` (clean
worktree, branch `feat/auto-20261116-7cc8b1dc`).

A read-only security audit on 2026-11-16 ran the repository's own
supply-chain gates against this baseline:

- `cargo audit --file Cargo.lock` — **exit 1**, three vulnerabilities.
- `cargo deny check` — **exit 1**, `advisories FAILED, bans ok, licenses ok,
  sources ok`.
- `cargo audit --file crates/eggserve-python/Cargo.lock` — exit 0, two
  unsound-code warnings.

Resolved versions at the baseline:

| Crate | Root `Cargo.lock` | Python `Cargo.lock` | Advisory |
|---|---|---|---|
| `quinn-proto` | `0.11.18` | absent | RUSTSEC-2026-0523, RUSTSEC-2026-0524 |
| `quinn-udp` | `0.5.15` | absent | RUSTSEC-2026-0538 |
| `rustix` | `1.1.4` | `1.1.4` | RUSTSEC-2026-0486 (unsound) |
| `tokio` | `1.52.3` | `1.53.1` | RUSTSEC-2026-0386 (unsound) |

The gate is the one documented in `docs/security-review.md` and enforced by
`scripts/check-supply-chain.sh:20-30`; it also runs daily in
`.github/workflows/advisory-scan.yml`.

## 3. Problem statement

The root advisory gate is red. The three vulnerabilities all sit in the
optional, explicitly experimental H3/QUIC closure:

- **RUSTSEC-2026-0523** — a peer-supplied `MAX_STREAM_DATA` frame can open
  streams beyond the advertised limit, exhausting memory or spinning the
  accept path. Servers that accept peer-initiated bidirectional streams are
  affected; patched in `quinn-proto >=0.11.19`.
- **RUSTSEC-2026-0524** — a long Retry token can panic or over-allocate in a
  QUIC **client**; no EggServe outgoing Quinn client was found. Patched in
  `quinn-proto >=0.11.19`.
- **RUSTSEC-2026-0538** — a truncated datagram can leave the receive loop
  spinning on affected Apple/BSD receive paths; patched in
  `quinn-udp >=0.5.16` (or `>=0.6.3`).

Quinn is reachable only through `crates/eggserve-h3`, which is gated by the
`http3` feature in `crates/eggserve-core/Cargo.toml` and
`crates/eggserve-bin/Cargo.toml`. The default H1/TLS graph does not contain
it. That bounds the exposure but does not make the gate acceptable: the
lockfile is distributed, the H3 adapter is published, and the repository's
own policy says the gate must be green.

The two unsound advisories are different: no in-tree call to `AsyncFd`
(Tokio) or to `rustix::fs` xattr functions exists, so they are not reachable
through EggServe APIs today. They still make `cargo deny check` fail and
should be raised while the lockfile is open.

## 4. Goal

Restore a green advisory gate by resolving every advisory-named patched
release in every lockfile that contains the affected crate, prove the
resolution and feature graph moved only by version, and add an offline
guard that fails if a vulnerable resolved version returns.

## 5. Non-goals

- No upgrade of `h3`, `h3-quinn`, or `quinn` beyond what the patched
  `quinn-proto`/`quinn-udp` require.
- No H3 tier promotion, capability change, or new H3 evidence claim.
- No publication, tag, or release.
- No change to TLS identity, HTTP framing, or runtime behavior.
- No dependency addition or removal.
- No advisory ignore entry in `deny.toml` or `audit.toml`.
- No `cargo update` of unrelated crates. Version drift must be attributable
  to an advisory.

## 6. Scope

### Work package A — Resolve the patched releases

Update the affected locked crates, using precise updates so unrelated
resolution cannot drift:

```sh
cargo update -p quinn-proto --precise 0.11.19
cargo update -p quinn-udp   --precise 0.5.16
cargo update -p rustix      --precise 1.1.6
cargo update -p tokio       --precise 1.53.3
```

Repeat for `crates/eggserve-python/Cargo.lock` (its own workspace) and
`fuzz/Cargo.lock` if the affected crates appear there. The QUIC crates are
expected to be absent from the Python and fuzz closures; verify rather than
assume.

The constraints already accept the patches:

- `crates/eggserve-h3/Cargo.toml` declares `quinn = "0.11.11"`, and
  `quinn 0.11.11` requires `quinn-proto = "0.11.15"` and `quinn-udp = "0.5"`.
- `Cargo.toml` declares `rustix = "1"` and `tokio = "1"`; the Python crate
  declares `tokio = "1"`.

If the local advisory database names a release that cannot be resolved
under these constraints, stop (see §14) rather than widening a constraint
silently.

### Work package B — Prove resolution neutrality

The only acceptable lockfile delta is the four crate versions and their
transitive checksums. Required evidence:

- `git diff -- Cargo.lock crates/eggserve-python/Cargo.lock fuzz/Cargo.lock`
  shows no package addition, removal, or unrelated version change.
- `cargo metadata --format-version 1 --locked` package list is identical
  except those versions.
- `cargo tree -e features --locked` before/after is identical except the
  version strings. In particular, the default H1/TLS graph still contains
  no `quinn*` package, and the `http3,tls` graph still contains the same
  feature set.

### Work package C — Add the offline resolved-version guard

Add a small offline guard (Python is the repository's gate language) that
reads the lockfiles and fails when a present crate is below its
advisory-named floor:

| Crate | Minimum |
|---|---|
| `quinn-proto` | `0.11.19` |
| `quinn-udp` | `0.5.16` |
| `rustix` | `1.1.6` |
| `tokio` | `1.53.3` |

Rules:

- A present `quinn-proto`/`quinn-udp` must meet the floor; their absence
  passes (H3 disabled).
- `rustix` and `tokio` are always present in both distributed closures and
  must meet the floor.
- The guard must not fetch the advisory database. It is a version check,
  not a replacement for `cargo audit`.
- Add a `--self-test` mutation suite: one fixture per crate below the
  floor, one at the floor, one above, one absent-quinn case, and one
  malformed-lockfile case. The self-test must fail if the comparison is
  inverted or the crate is skipped.
- Wire it into `scripts/check-supply-chain.sh` **before** the network
  audits, and into `scripts/verify.sh fast` if that scope remains cheap
  and offline. Record the exact wiring.

Do not put advisory handling in `deny.toml`; that file owns bans/licenses,
and its `ignore = []` policy is the correct one.

### Work package D — Update the dependency and H3 documentation

- `docs/dependency-policy.md`: record the four advisory floors and the
  guard, next to the existing `rustls` floor description.
- `architecture/eggserve-h3.md`: state the minimum `quinn-proto`/
  `quinn-udp` releases and that the H3 adapter follows the QUIC patched
  line; do not imply an H3 tier change.
- `AGENTS.md` and `.opencode/skills/eggserve-dev/SKILL.md` only if they
  name the old versions or claim the gate is green; keep the edit factual.
- `SECURITY.md` only if its advisory-response description needs the new
  guard named.

### Work package E — Qualify the H3 closure without over-claiming

- `cargo check -p eggserve-h3 --all-targets --locked`
- `cargo test -p eggserve-h3 --locked`
- `cargo test -p eggserve-core --test http3_runtime --features http3,tls
  --locked` (or the exact current H3 integration target)
- `cargo check --workspace --all-targets --features http3,tls --locked`

`scripts/qualify-http3.sh` remains a manual, independent-client lane. Do
not claim H3 wire evidence from unit tests, and do not promote the tier.

## 7. Invariants that must not regress

1. **No vulnerable locked version.** `quinn-proto <0.11.19`,
   `quinn-udp <0.5.16`, `rustix <1.1.6`, or `tokio <1.53.3` must never
   resolve in a distributed lockfile.
2. **No advisory silence.** `deny.toml` and any audit configuration keep
   `ignore = []`; the fix is a version, not an exception.
3. **The default graph stays QUIC-free.** H3 remains opt-in behind
   `http3`; the default build and the Python wheel do not acquire QUIC.
4. **No feature-set drift.** The patched versions must not enable a new
   default feature (notably an alternate crypto provider or 0-RTT).
5. **H3 stays experimental.** This plan restores a security floor; it does
   not supply independent-client, adversarial-wire, or cross-platform H3
   evidence.
6. **No publication.** Resolution changes are consumed by the next release
   the maintainer chooses; this plan does not publish.

## 8. Failure, cancellation, restart, and contention semantics

- **Network/registry unavailable** during `cargo update` or the advisory
  database fetch: record the exact failing command and leave the gate red;
  do not hand-edit `Cargo.lock` and do not claim the advisory is fixed.
- **Unresolvable patch** (MSRV, `h3-quinn` coupling, or a required `h3`
  upgrade): stop and report. A forced upgrade of the H3 protocol crates is
  a different milestone, not a dependency bump.
- **Partial update** (e.g. `quinn-proto` moves, `quinn-udp` does not):
  leave the tree uncommitted, record which crate failed, and do not claim
  a green gate.
- **Advisory database expects a newer release than crates.io can serve**:
  record the gap explicitly; the guard floor and the advisory's patched
  release must agree.

## 9. Compatibility and migration

- `quinn-proto` and `quinn-udp` are transitive, non-public dependencies of
  the experimental `eggserve-h3` adapter. No public Rust or Python API
  changes.
- `rustix` and `tokio` are patch-level updates within their caret lines.
  No feature or MSRV change is expected; verify MSRV 1.89 still holds.
- Consumers of the published crates are unaffected until a publication;
  this plan does not force one.
- If a patch release raises MSRV above 1.89, stop: the workspace MSRV is a
  release constraint (`AGENTS.md`).

## 10. Required tests

### Focused

- Guard unit/self-test mutation matrix (below-floor, at-floor, above-floor,
  absent-quinn, malformed input).
- Version assertions for both lockfiles.

### Integration

- `cargo test -p eggserve-h3 --locked`.
- The H3 integration target under `http3,tls`.
- `cargo check --workspace --all-targets --features http3,tls --locked`.

### Security and negative

- `cargo audit --file Cargo.lock` exits 0.
- `cargo audit --file crates/eggserve-python/Cargo.lock` exits 0.
- `cargo deny check` exits 0 (root) and the Python-crate deny invocation
  exits 0.
- The guard fails closed on each seeded vulnerable version.
- `git diff` contains no `ignore =` advisory exception.

### Compatibility

- `cargo tree -e features --locked` feature-set diff is empty except
  version strings.
- Default graph contains no `quinn*`.
- MSRV check: `cargo +1.89 check --workspace --all-targets --locked`.

## 11. Required verification commands

```bash
# after the precise updates
cargo metadata --format-version 1 --locked > /tmp/metadata-after.json
cargo tree -e features --locked > /tmp/features-after.txt
git diff -- Cargo.lock crates/eggserve-python/Cargo.lock fuzz/Cargo.lock

# offline guard
python3 scripts/<new-guard>.py --self-test
python3 scripts/<new-guard>.py

# repository gates
bash scripts/check-supply-chain.shcargo +1.89 check --workspace --all-targets --locked
cargo check --workspace --all-targets --features http3,tls --locked
cargo test -p eggserve-h3 --locked
cargo test -p eggserve-core --test http3_runtime --features http3,tls --locked
./scripts/verify.sh fast
```

`cargo audit` and `cargo deny` require the advisory database; the guard
must run without it. `./scripts/verify.sh full` is optional here because it
adds TLS/examples/wheel work unrelated to the resolution; run it only if
the changed manifests affect the wheel closure.

## 12. Documentation updates

- `docs/dependency-policy.md` — advisory floors + guard.
- `architecture/eggserve-h3.md` — patched QUIC minimums, tier unchanged.
- `plans/subsystems/cli-ops-observability-roadmap.md` — milestone status.
- `plans/registry.md` — status and closure pointer.

## 13. Acceptance criteria

- Both distributed lockfiles resolve `quinn-proto >=0.11.19`,
  `quinn-udp >=0.5.16`, `rustix >=1.1.6`, and `tokio >=1.53.3` where the
  crate is present.
- `cargo audit` on both lockfiles exits 0 with no vulnerability, and
  `cargo deny check` exits 0 in both closures.
- The offline guard fails closed on every seeded regression and passes on
  the fixed tree.
- The default feature graph still contains no QUIC crate; the `http3,tls`
  feature graph changes only by version.
- H3 tests pass; the H3 tier and its evidence claims are unchanged.
- No advisory ignore entry exists.

## 14. Stop conditions

Stop and report rather than improvise when:

- a patched release cannot be resolved within the declared constraints;
- the patch requires an `h3`/`h3-quinn`/`quinn` major-version change;
- the patch raises MSRV above 1.89;
- the advisory database or registry is unavailable;
- the feature graph changes beyond version strings;
- the guard cannot be made deterministic and offline.

## 15. Closure evidence required

- Exact `cargo update` commands and the resulting lockfile diffs.
- Before/after `cargo metadata` and `cargo tree -e features` deltas.
- `cargo audit` and `cargo deny` outputs for both lockfiles (exit 0).
- Guard self-test output and one deliberate regression failure.
- H3 test/check outputs.
- MSRV check output.
- Remaining risk: H3 remains experimental; future advisories still require
  the daily scan.

## 16. Handoff notes

- Do not use `cargo update` without `--precise`; unrelated drift would make
  the feature-diff evidence meaningless.
- The local advisory snapshot may be newer or older than CI's; record the
  advisory IDs and database freshness. A green local audit is not a
  substitute for the hosted scheduled scan.
- `fuzz/Cargo.lock` is a separate workspace. Check it, but do not update it
  for crates it does not contain.
- Tool-version drift is a real trap: the repository pins `cargo-deny 0.19.0`
  in `scripts/install-cargo-tools.sh`, where `--config` is a `check`
  subcommand option. A locally installed 0.20.x rejects that placement.
  Trust the pinned CI tool, not a newer local one; if the pin is ever
  raised, re-verify the invocation rather than assuming it is unchanged.
- `quinn-udp 0.6.x` is also patched; stay on the `0.5` line unless
  `quinn 0.11.11` requires otherwise. A `0.6` move is a different
  dependency decision.
