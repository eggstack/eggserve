# Plan 323 — Post-audit operator and repository hygiene sweep

## 1. Status

- **State**: ready for handoff
- **Plan number**: 323
- **Subsystem**: CLI, ops, and observability
  (`plans/subsystems/cli-ops-observability-roadmap.md`, Milestone 323)
- **Source roadmap**: `plans/subsystems/cli-ops-observability-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (operator-visible diagnostics;
    safe defaults)
  - `plans/001-terminology-and-domain-model.md` (CLI surface)
  - `plans/003-planning-process.md` (polish classification and verification)
- **Primary class**: polish (operator docs, dependency hygiene, CI/release
  reproducibility, test correctness)

## 2. Repository baseline

Repository baseline: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

A security audit found a set of Low/Informational items that are not
vulnerabilities but are inaccurate or under-pinned. This plan is a
corrective sweep, not a capability milestone. Each work package is
independently closable; the closure record may mark a package blocked
without blocking the others only if the package is explicitly recorded.

## 3. Problem statement

The audited items:

1. **`--public` documentation contradicts the CLI.** The flag only
   acknowledges an explicit wildcard bind; it does not change the bind
   address (`crates/eggserve-bin/src/args.rs:251`, `410-416`, `768`). But
   `docs/deployment.md:196-202` says `eggserve --public --port 8000
   --directory public` “binds to `0.0.0.0`”, and `docs/cli.md:23` calls
   the flag “Bind to all interfaces”. The current behavior fails safe, but
   the documented example does not do what it says.
2. **`audit.toml` is at best inert for the policy it appears to state.**
   `scripts/check-supply-chain.sh` passes no config file to `cargo audit`
   and uses `deny.toml` for `cargo deny`; the root `audit.toml` carries
   cargo-deny schema (`[bans] wildcards = "allow"`, `[licenses]`) while
   `docs/dependency-policy.md:270` says it configures `cargo audit`
   defaults.
3. **Core enables an unused `tokio/signal` feature.**
   `crates/eggserve-core/Cargo.toml:65` enables `signal`; only
   `crates/eggserve-bin/src/shutdown.rs:8-27` uses signal APIs.
   `docs/dependency-policy.md` explicitly lists core as having no
   `signal`.
4. **`.gitignore` has no credential-file patterns.** No secret was found
   in the current checkout, but `.env`, private keys, and local
   certificates are not ignored.
5. **Release qualification uses mutable container tags.**
   `.github/workflows/release.yml:270, 283, 537, 559-561` use
   `python:3.11-alpine` and `arm32v7/python:3.11-*` without digests.
6. **`docs/action-pinning.md` has a stale entry.** It lists
   `actions/cache`; no workflow uses it. The actual inventory must match.
7. **`docs/threat-model.md` opens with a corrupted paragraph.** The
   sentence fragments “A custom Rust or Python service cannot cause
   static-root escape.” and “construction merely by starting.” lost a
   connecting line (`docs/threat-model.md:3-7`).
8. **A static test fails on macOS for a test-only reason.**
   `fs::tests::resolve_child_directory_retains_canonical_path` compares
   the resolved canonical path (`/private/var/...`) with a non-canonical
   temp path (`/var/...`) at `crates/eggserve-static/src/fs/mod.rs:736`.
   The product is correct; the test is brittle.
9. **Release hashes are integrity-only, not authenticated provenance.**
   `release.yml:363-380` emits a SHA-256 `MANIFEST` in the same workflow
   that uploads the wheels. This is useful but not independent
   attestation, and no document says so.

## 4. Goal

Make the operator-facing documentation match the CLI, remove the dead
dependency/config surfaces, pin release inputs where feasible, and restore
the accuracy of the security/threat documentation and the platform test
suite.

## 5. Non-goals

- **No `--public` behavior change.** The fail-safe behavior (acknowledge
  only) stays; the docs are corrected. A behavior change would be a
  separate, security-relevant plan.
- No new release-attestation infrastructure (SLSA/provenance). This plan
  records the existing MANIFEST limitation; adding attestation is a
  separate milestone if the operator wants it.
- No dependency version change other than removing the unused feature.
- No change to `deny.toml` policy or advisory ignores.
- No confinement behavior change (the macOS item is test-only).
- No broad documentation rewrite; only the named inaccuracies.

## 6. Scope

### Work package A — `--public` documentation and behavior pin

- Correct `docs/cli.md` (table and prose), `docs/deployment.md`
  (the public-bind example), `docs/security-policy.md` (safe-defaults
  row and the `--public` section), and `docs/security-review.md` (safe
  defaults table) to state: `--public` acknowledges an explicit wildcard
  `--bind`/`--addr`; it does not change the bind address.
- Replace the deployment example with
  `eggserve --directory public --public --addr 0.0.0.0:8000`.
- Add a CLI unit test pinning the behavior:
  `parse(&["--public"]).unwrap().bind.ip().is_loopback()` and, if a live
  bind test exists, a loopback assertion for the flag alone.
- Search `README.md` and examples for the same implication.

### Work package B — Audit configuration truth

- Establish what `cargo audit` actually reads at this version. Its CLI
  exposes no config-file flag; verify the installed release's documented
  configuration path (commonly `.cargo/audit.toml`) rather than assuming
  the root file is read.
- Choose one:
  - move the supported `[advisories] ignore = []` / `[yanked]` subset to
    `.cargo/audit.toml` and delete the root `audit.toml`; or
  - keep a root file only if a verified `cargo audit` invocation reads it,
    and remove the cargo-deny-schema sections that do not belong.
- Update `docs/dependency-policy.md:270` and `SECURITY.md` to name the
  single effective policy source (`deny.toml` for deny, the audit config
  for audit).
- Do not change `deny.toml` `wildcards = "deny"`.

### Work package C — Remove the unused `tokio/signal`

- Remove `"signal"` from the `tokio` feature list in
  `crates/eggserve-core/Cargo.toml:65`.
- Confirm `crates/eggserve-bin` still enables `signal` for shutdown.
- Prove the core feature graph no longer contains `signal` or
  `signal-hook-registry`:
  `cargo tree -e features -p eggserve-core | grep -E 'signal'` is empty.
- Update `docs/dependency-policy.md` only if the table already says “no
  signal” (it does); the code now matches it.

### Work package D — Credential-file ignore patterns

- Add defensive patterns to `.gitignore`, chosen so intentional test
  fixtures are not hidden. A tracked fixture exists at
  `crates/eggserve-python/tests/fixtures/localhost-test.key`, so a blanket
  `*.key` rule needs an explicit negation such as
  `!crates/eggserve-python/tests/fixtures/*.key` (or the fixture should be
  renamed to a non-secret extension). Verify tracked files first
  (`git ls-files '*.pem' '*.key' '*.p12' '*.pfx'`).
- Suggested: `.env`, `.env.*`, `*.p12`, `*.pfx`, `id_rsa*`,
  `id_ed25519*`, `*.key` plus the fixture negation.
- Do not add `*.pem` blindly if tracked PEM fixtures exist; record the
  decision.

### Work package E — Pin release container images by digest

- For each mutable image reference in `.github/workflows/release.yml`
  (`python:3.11-alpine`, `arm32v7/python:3.11-bookworm`,
  `arm32v7/python:3.11-alpine`), resolve the current manifest-list digest
  and pin it (`image@sha256:...`).
- Add a short “image digest update” procedure next to the existing action
  pinning procedure.
- If the execution environment has no registry access, record the exact
  blocked command and leave the tag in place; do not invent a digest.
- Do not change the runner OS, QEMU setup, or wheel matrix.

### Work package F — Action-pinning inventory accuracy

- Sync `docs/action-pinning.md` with the actual `uses:` set in
  `.github/workflows/*.yml`.
- Remove `actions/cache` if it is still unused; add any used action that
  is missing.
- Confirm every actual `uses:` is a full 40-hex SHA. This is a docs-only
  change unless a workflow is found unpinned (then stop and report).

### Work package G — Restore the threat-model paragraph

- Use `git log -p -- docs/threat-model.md` to recover the lost sentence.
  If the original text is unrecoverable, rewrite the opening paragraph
  so it states the complete intended thought: static state carries
  confinement capabilities only; transport owns the file-stream pool and
  final headers; a custom service cannot cause root escape merely by
  starting.
- Preserve the paragraph's meaning; do not introduce a new claim.
- Check the same paragraph's neighbors for other line-loss artifacts.

### Work package H — macOS test canonicalization

- In `crates/eggserve-static/src/fs/mod.rs`, compare the resolved child
  path against `fs::canonicalize(tmp.path()).join("subdir")` (or compare
  canonicalized forms), not the raw temp path.
- This is a test-only change; no production path changes.
- Run `cargo test -p eggserve-static` on macOS and Linux (the hosted macOS
  compile lane already exists; the test lane may be Linux-only).

### Work package I — Release manifest provenance wording

- Add a note to `docs/release-contract.md` (or the release workflow
  documentation) that the SHA-256 `MANIFEST` is integrity-comparison
  data produced by the same workflow, not independently signed
  provenance.
- Do not add attestation in this plan. If the operator wants it, a new
  milestone with the required `id-token`/`attestations` permissions and
  verification steps is the right vehicle.

## 7. Invariants that must not regress

1. **`--public` remains fail-safe.** A wildcard bind still requires the
   acknowledgment; the flag alone still binds loopback.
2. **`deny.toml` policy is unchanged.** Wildcards denied; `native-tls`/
   `openssl-sys` banned; no advisory ignores.
3. **No runtime behavior change** except the removed unused Tokio feature
   (dependency graph only) and the test-only path fix.
4. **Release inputs become more pinned, not less.** If a digest cannot be
   resolved, leaving the tag and recording it is preferable to a guessed
   digest.
5. **Documentation claims are executable-source-backed.** Do not update a
   doc to a claim the code does not implement.
6. **No secrets are added.** The ignore patterns are defensive only.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable to most packages. For the release-digest package, a failed
registry lookup must leave the workflow unchanged and the package blocked,
not half-pinned. For the `.gitignore` package, verify that existing
tracked fixtures remain tracked (ignore patterns do not affect tracked
files) and that `git status` does not hide an intentional new fixture.

## 9. Compatibility and migration

- `--public` docs change only; no command-line behavior change.
- Removing `tokio/signal` from core is a feature-graph change, not a
  public feature removal (the feature is a Tokio passthrough, not an
  EggServe feature).
- The macOS test does not affect consumers.
- Container digest pinning affects only release qualification inputs.
- Audit-config cleanup affects maintainers only.

## 10. Required tests

### Focused

- CLI parser tests for `--public` + no bind (loopback) and `--public` +
  wildcard bind (accepted).
- `cargo test -p eggserve-static` on macOS for the canonicalization fix.
- `python3 scripts/check-crate-topology.py` and `--self-test`.

### Integration

- `cargo test -p eggserve-bin` (CLI).
- `cargo check --workspace --all-targets`.
- `cargo tree -e features -p eggserve-core` signal absence.
- Workflow YAML parse/validation (existing release-workflow self-test if
  it covers image refs; otherwise a syntax check).

### Security and negative

- `cargo deny check` remains green; `deny.toml` diff is empty.
- `git ls-files` confirms no newly ignored tracked fixture disappeared.
- The action-pinning audit finds every `uses:` at 40 hex; no
  `pull_request_target`/`workflow_run` is introduced.

## 11. Required verification commands

```bash
cargo test -p eggserve-bin
cargo test -p eggserve-static
cargo check --workspace --all-targets
cargo tree -e features -p eggserve-core | grep -E 'signal' || true
python3 scripts/check-crate-topology.py --self-test
python3 scripts/check-crate-topology.py
./scripts/verify.sh fast
git ls-files '*.pem' '*.key' '*.p12' '*.pfx' .env '.env.*'
```

Run the static suite on macOS for Work package H; the Linux CI suite is
the broad gate. `./scripts/verify.sh full` is optional and requires Python
3.14 + maturin.

## 12. Documentation updates

- `docs/cli.md`, `docs/deployment.md`, `docs/security-policy.md`,
  `docs/security-review.md` (`--public`).
- `docs/dependency-policy.md` and `SECURITY.md` (audit config, core
  signal feature).
- `.gitignore` (credential patterns).
- `docs/action-pinning.md` (inventory).
- `docs/threat-model.md` (restored paragraph).
- `docs/release-contract.md` (manifest provenance wording).
- `CHANGELOG.md` for the signal-feature and any visible release-input
  change.
- Roadmap/registry status.

## 13. Acceptance criteria

- `--public` docs and tests agree with the actual CLI: the flag alone
  keeps loopback; a wildcard bind requires it.
- The audit policy has one documented source per tool; no dead
  cargo-deny-schema file is presented as `cargo audit` configuration.
- `eggserve-core` no longer enables `tokio/signal`.
- Credential-file patterns are present without hiding tracked fixtures.
- Release images are digest-pinned or explicitly recorded as blocked with
  the failed lookup.
- The action-pinning inventory matches the workflows.
- `docs/threat-model.md` reads as a complete paragraph.
- `cargo test -p eggserve-static` passes on macOS.
- The manifest-provenance limitation is documented.
- No behavior change beyond the dependency-graph feature removal and the
  test-only fix.

## 14. Stop conditions

Stop and report rather than improvise when:

- a docs correction would require changing `--public` behavior;
- `cargo audit`'s configuration behavior cannot be established;
- removing `tokio/signal` from core breaks a core build or test;
- a registry digest cannot be resolved (record blocked, leave the tag);
- the `.gitignore` change would hide a tracked test fixture;
- the action-pinning audit finds an unpinned action (report, do not
  invent a SHA).

## 15. Closure evidence required

- Docs and test diffs for each work package.
- `cargo test -p eggserve-bin` and macOS `cargo test -p eggserve-static`
  outputs.
- `cargo tree -e features -p eggserve-core` signal-absence output.
- The audit-config decision and the effective command for each tool.
- The image-digest resolution output, or the recorded blocked lookup.
- The action inventory before/after.
- The restored threat-model paragraph and its Git-history source.
- Confirmation that `deny.toml` is unchanged.

## 16. Handoff notes

- Keep each work package independent; a blocked digest or audit-tool
  question must not hold the `--public` or threat-model corrections
  hostage.
- `docs/deployment.md` is a user-facing normative doc; correct it toward
  the safe behavior, not toward a behavior change.
- Do not add `*.pem` to `.gitignore` without checking tracked fixtures;
  TLS tests may generate files or commit fixtures.
- The release workflow is a manual, credentialed surface; do not trigger
  it as part of this plan.
- The macOS test failure is environment-specific (`/private` symlink); a
  Linux-only run will not reproduce it. Record which platform ran which
  test.
