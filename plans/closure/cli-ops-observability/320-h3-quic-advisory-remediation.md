# CLI, Ops, and Observability Milestone 320 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/320-h3-quic-advisory-remediation.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`
(worktree `feat/auto-20261116-7cc8b1dc`).

Implementation state: the change is in the working tree. No commit, tag, or
publication was created by this task.

## 1. Executive finding

The root advisory gate is green again. Every advisory-named crate was moved to
its patched release with `cargo update --precise`, in every lockfile that
contains it:

| Crate | Before | After | Advisory |
|---|---|---|---|
| `quinn-proto` | 0.11.18 | **0.11.19** | RUSTSEC-2026-0523, RUSTSEC-2026-0524 |
| `quinn-udp` | 0.5.15 | **0.5.16** | RUSTSEC-2026-0538 |
| `rustix` | 1.1.4 | **1.1.6** | RUSTSEC-2026-0486 (unsound) |
| `tokio` | 1.52.3 (root/fuzz), 1.53.1 (Python) | **1.53.3** | RUSTSEC-2026-0386 (unsound) |

Resolution moved only by version. `cargo metadata` package sets differ by
exactly those four name/version pairs; the normalized `cargo tree -e features`
output is identical for the default workspace and for the `http3,tls` graph.
The H3 protocol crates did not move (`quinn 0.11.11`, `h3-quinn 0.0.10`,
`h3 0.0.8`). The default `eggserve-bin` and `eggserve-core` graphs still
contain no `quinn*` package.

A new offline guard, `scripts/check-advisory-floors.py`, fails closed when any
of the four crates resolves below its floor. It reads the root, Python, and
fuzz lockfiles, needs no advisory database, and ships a `--self-test` mutation
matrix. It runs in `scripts/check-supply-chain.sh` before the network audits
and in `scripts/verify.sh fast`.

H3 remains experimental. No tier, capability, or evidence claim changed.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| Resolve every patched release with precise updates | met | §3.1 commands; `git diff -- Cargo.lock` shows 4 versions + checksums |
| Python and fuzz lockfiles checked and updated | met | Python: `rustix 1.1.6`, `tokio 1.53.3`; fuzz: same. QUIC crates absent from both |
| Resolution/feature neutrality | met | §3.2 metadata and tree deltas |
| Offline resolved-version guard with self-test | met | `scripts/check-advisory-floors.py`, `--self-test` 20 checks |
| Guard wired before network audits and into fast verification | met | `scripts/check-supply-chain.sh`; `scripts/verify.sh::cmd_fast` |
| No advisory ignore entry | met | `deny.toml` and `.cargo/audit.toml` both keep `ignore = []` |
| Docs updated (dependency policy, H3, SECURITY) | met | §7 |
| H3 qualification without over-claiming | met | §4 H3 test/check outputs; no tier change |
| No dependency addition/removal | met | metadata package delta is version-only |
| MSRV 1.89 preserved | met | `cargo +1.89 check --workspace --all-targets --locked` |
| No publication | met | none attempted |

## 3. Implementation evidence

### 3.1 Exact updates

```sh
cargo update -p quinn-proto --precise 0.11.19
cargo update -p quinn-udp   --precise 0.5.16
cargo update -p rustix      --precise 1.1.6
cargo update -p tokio       --precise 1.53.3
# Python crate (own workspace, run outside the nested worktree):
cargo update -p rustix --precise 1.1.6
cargo update -p tokio  --precise 1.53.3
# fuzz workspace:
cargo update -p rustix --precise 1.1.6
cargo update -p tokio  --precise 1.53.3
```

Root lockfile delta: the four versions and their checksums, plus the
`windows-sys` dependency edge for `quinn-proto`, `quinn-udp`, and `rustix`
moving from `0.61.2` to the already-present `0.52.0` (upstream relaxed the
requirement in the patched releases). Both `windows-sys` packages remain in the
lockfile; no package was added or removed. The Python lockfile moves only the
two versions and checksums. The fuzz lockfile's eventual
`signal-hook-registry` removal belongs to Plan 323's core-feature change, not
to this resolution; the advisory update itself moved only `rustix` and
`tokio` there.

### 3.2 Neutrality evidence

- `cargo metadata --format-version 1 --locked` (before: clean HEAD worktree;
  after: this worktree): added/removed package identities are exactly the four
  old/new version pairs; no other crate name changed version.
- `cargo tree --workspace -e features --locked`, normalized for version strings
  and worktree paths: identical before/after.
- `cargo tree -p eggserve-core --features http3,tls -e features --locked`,
  same normalization: identical.
- `cargo tree -p eggserve-bin --locked -e normal | grep -c quinn` → `0`;
  `-p eggserve-core` → `0`; with `--features http3,tls` → `5`.

### 3.3 Offline guard

- Live check: `ok [root] … quinn-proto 0.11.19, quinn-udp 0.5.16,
  rustix 1.1.6, tokio 1.53.3`; `ok [python] …`; `ok [fuzz] …`.
- `--self-test`: `self-test: 20 checks passed`.
- Deliberate mutation 1 (comparison inverted): 9 self-test failures, exit 1.
- Deliberate mutation 2 (`quinn-proto` skipped): `below floor` case fails,
  exit 1.
- Malformed lockfile, missing lockfile, and missing required `rustix`/`tokio`
  cases all fail closed.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo audit --file Cargo.lock` | PASS (exit 0, 182 deps) |
| `cargo audit --file crates/eggserve-python/Cargo.lock` | PASS (exit 0, 84 deps) |
| `cargo audit --file fuzz/Cargo.lock` | PASS (exit 0, 58 deps) |
| `cargo deny check` (root) | PASS — `advisories ok, bans ok, licenses ok, sources ok` |
| `cargo deny check --config ../../deny.toml` (Python crate) | PASS in a non-nested checkout — `advisories ok, bans ok, licenses ok, sources ok` |
| `python3 scripts/check-advisory-floors.py --self-test` | PASS — 20 checks |
| `python3 scripts/check-advisory-floors.py` | PASS — all three lockfiles |
| `cargo +1.89 check --workspace --all-targets --locked` | PASS |
| `cargo check --workspace --all-targets --features http3,tls --locked` | PASS |
| `cargo test -p eggserve-h3 --locked` | PASS — 13 tests + doc-tests |
| `cargo test -p eggserve-core --test http3_runtime --features http3,tls --locked` | PASS — 16 tests |
| `bash scripts/check-supply-chain.sh` | PASS end-to-end in a standalone checkout: offline guard, root audit/deny, Python audit/deny, both `advisories ok, bans ok, licenses ok, sources ok`. The nested worktree cannot run the Python deny step (`cargo metadata` resolves the parent repository); that is a checkout-layout limitation, recorded in §6 |

## 5. Invariant review

| Invariant | Result |
|---|---|
| No vulnerable locked version | Held; guard and audits pass on all lockfiles |
| No advisory silence | Held; both ignore lists are empty, no `#[ignore]` added |
| Default graph stays QUIC-free | Held; `eggserve-bin`/`eggserve-core` default trees contain no `quinn*` |
| No feature-set drift | Held; normalized feature trees are identical; no new default feature, 0-RTT unchanged |
| H3 stays experimental | Held; no tier or evidence claim changed |
| No publication | Held |

## 6. Failure and recovery review

- The Python crate's `cargo deny` invocation cannot be executed from this
  nested worktree because `cargo metadata` resolves the parent repository's
  `Cargo.toml`; that is a checkout-layout limitation, not a script defect.
  The exact pinned invocation is valid: `cargo-deny 0.19.0` defines `--config`
  on the `check` subcommand. The same command was executed in a standalone
  checkout (`/tmp`, outside the parent tree) and exited 0.
- A stricter local `cargo-deny 0.20.2` rejects the pinned placement; per the
  plan, the script was not changed for a newer local tool. This is recorded as
  tool-version drift in the plan's handoff notes.
- No hand-edited lockfile content, no advisory ignore, no partial update.

## 7. Documentation and operations

| Surface | Change |
|---|---|
| `docs/dependency-policy.md` | Advisory-named floors + offline guard; guard is not a database replacement |
| `architecture/eggserve-h3.md` | Quinn patched minimums; tier unchanged |
| `SECURITY.md` | Offline floor guard named before the network audits |
| `scripts/check-advisory-floors.py` | New offline guard + self-test |
| `scripts/check-supply-chain.sh` | Guard runs before the network audits |
| `scripts/verify.sh` | Guard runs in `fast` |
| `CHANGELOG.md` | Unreleased entry |
| `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md` | Checked, not edited: they name no pre-fix quinn/rustix/tokio version and claim no green gate |

## 8. Residual risk

H3 remains experimental and its exposure was bounded by the optional `http3`
feature, but the lockfile is distributed, so the advisory fix still mattered.
Future advisories still require the daily hosted scan; the offline guard only
knows the floors written in this repository.

## 9. Roadmap disposition

`plans/subsystems/cli-ops-observability-roadmap.md` Milestone 320 is closed.
Publication of the resolved line remains a separate maintainer action.

## 10. Registry updates

`plans/registry.md` row 320 moves to `closed` with a pointer to this record;
the CLI/ops roadmap status row and milestone table are updated to match.
