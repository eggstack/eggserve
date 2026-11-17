# CLI, Ops, and Observability Milestone 323 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/323-post-audit-operator-and-repository-hygiene-sweep.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

Implementation state: the change is in the working tree. No commit was created
by this task. Every work package landed; none is blocked.

## 1. Executive finding

The audited Low/Informational items are corrected without a runtime behavior
change. `--public` remains fail-safe and its documentation now matches the
parser. The dead root `audit.toml` is replaced by the file cargo-audit actually
reads. Core no longer enables an unused Tokio feature. Credential ignore
patterns are present with the tracked fixture re-included. Release images are
digest-pinned. The action inventory matches the workflows. The corrupted
threat-model opening is restored from Git history. The macOS-only static test
now compares canonical forms. The release MANIFEST limitation is documented.

No `--public` behavior change, no `deny.toml` change, no new release
attestation, and no dependency version change were made.

## 2. Requirement-to-evidence matrix

| Work package | Status | Evidence |
|---|---|---|
| A — `--public` docs + behavior pin | met | §3.1; 4 doc surfaces + examples; new parser test |
| B — audit configuration truth | met | §3.2; `.cargo/audit.toml`; root `audit.toml` deleted |
| C — remove unused `tokio/signal` | met | §3.3; feature-tree evidence |
| D — credential ignore patterns | met | §3.4; fixture negation verified |
| E — release image digests | met | §3.5; three digests pinned, YAML parses |
| F — action-pinning inventory | met | §3.6; `actions/cache` removed; no unpinned `uses:` |
| G — threat-model paragraph | met | §3.7; restored from `455fb5c^` |
| H — macOS static test | met | §3.8; full static suite green on macOS |
| I — manifest provenance wording | met | §3.9; release-contract subsection |
| No behavior change beyond the feature removal and test fix | met | §5 invariant review |

## 3. Implementation evidence

### 3.1 `--public`

The parser only checks `!public && bind_ip.is_unspecified()`
(`crates/eggserve-bin/src/args.rs`); it never rewrites the bind address.
Documentation now says so in `docs/cli.md`, `docs/deployment.md`,
`docs/security-policy.md`, `docs/security-review.md`, and
`docs/release-contract.md`. The deployment example and `examples/README.md`
use `--public --addr 0.0.0.0:<port>`. New test:

```rust
#[test]
fn public_flag_alone_keeps_loopback_bind() {
    let args = parse(&["--public"]).unwrap();
    assert!(args.bind.ip().is_loopback());
    assert_eq!(args.bind.port(), 8000);
}
```

### 3.2 Audit configuration

cargo-audit 0.22.2's `config_path()` resolves `./.cargo/audit.toml` or
`$CARGO_HOME/audit.toml`; it does not read a repository-root `audit.toml`.
Evidence that the new file is read: appending an unknown key made
`cargo audit` fail with a TOML parse error (exit 1), and the unmodified file
scans cleanly (exit 0).

`.cargo/audit.toml` carries `[advisories] ignore = []` and
`[yanked] enabled = true` (the 0.22 schema; the old `[yanked] version = "warn"`
key belongs to a different tool and is rejected by `deny_unknown_fields`).
The root `audit.toml` (mixed cargo-deny schema) is deleted.
`docs/dependency-policy.md` and `SECURITY.md` name one policy source per tool.

### 3.3 `tokio/signal`

Removed from the production `tokio` feature list in
`crates/eggserve-core/Cargo.toml`. Evidence:

- `cargo tree -p eggserve-core -e features,no-dev --no-default-features |
  grep -cE 'feature "signal"|signal-hook-registry'` → `0`.
  (Plain `-e features` includes dev edges and therefore the example's
  signal feature.)
- `cargo tree -p eggserve-bin -e features,no-dev` still matches both (bin
  owns the handler in `shutdown.rs`).
- `cargo tree -p eggserve-core -e features,dev --no-default-features` shows
  the dev-only signal edge used by the examples.

Six core examples call `tokio::signal::ctrl_c()`. The first verification run
failed `cargo +1.89 check -p eggserve-core --all-targets` because the library
feature was their only signal source. The fix keeps the production graph
signal-free and adds `signal` to the existing **dev-dependency** on `tokio`
with a comment: examples own Ctrl-C handling, embedders do not inherit a
signal driver. This is not a dependency addition (same crate).

One lockfile consequence: `eggserve-fuzz` path-depends on core, so
`fuzz/Cargo.lock` no longer needs `signal-hook-registry` and a plain
`cargo check --manifest-path fuzz/Cargo.toml --bins` regenerated it (4
insertions, 15 deletions; one package removed). The root and Python lockfiles
keep the package because `eggserve-bin` and the Python runtime still enable
the feature. `cargo check --manifest-path fuzz/Cargo.toml --bins --locked` now
passes.

### 3.4 Credential patterns

Added `.env`, `.env.*`, `!.env.example`, `*.p12`, `*.pfx`, `id_rsa*`,
`id_ed25519*`, `*.key`, and
`!crates/eggserve-python/tests/fixtures/*.key`.

Verified with `git check-ignore --no-index`:

| Path | Result |
|---|---|
| `.env` | ignored |
| `.env.local` | ignored |
| `foo.key` | ignored |
| `id_rsa` | ignored |
| `crates/eggserve-python/tests/fixtures/localhost-test.key` | **not ignored** (tracked fixture) |
| a new `fixtures/probe.key` | appears in `git status` (not hidden) |

`*.pem` was deliberately not added: no tracked PEM fixture exists today, and a
blanket rule could hide an intentional TLS fixture. This decision is recorded
here.

### 3.5 Release images

Resolved through the Docker Hub registry API (manifest-list digest) and pinned
in `.github/workflows/release.yml`:

| Image | Digest |
|---|---|
| `python:3.11-alpine` | `sha256:7fbad8e4652eae0186429a6f26cf5b23e27e8c6fd7ac4d0cfd7a6988cdeea1e1` |
| `arm32v7/python:3.11-bookworm` | `sha256:f5ac5bd76de95eeadfd7fc35d5385e6fa14fa3ed6ce070ecacbfb935eec47c66` |
| `arm32v7/python:3.11-alpine` | `sha256:c7b37e6338824a66959195b7657a7bd0c6e5020d51b49f295b27fd9af95dd227` |

`docs/action-pinning.md` gains the digest table and the resolution procedure.
The workflow still parses as YAML (PyYAML `safe_load`). No runner, QEMU, or
wheel-matrix change.

### 3.6 Action inventory

The only documented/actual mismatch was `actions/cache`, which no workflow
uses; its row is removed. `grep -rn 'uses:' .github/workflows/ |
grep -v '@[a-f0-9]\{40\}'` returns nothing. Every actual `uses:` is a full
40-hex SHA.

### 3.7 Threat-model paragraph

`git show 455fb5c^:docs/threat-model.md` holds the pre-corruption sentence
`A custom Rust or Python service cannot cause static-root construction merely
by starting.` The commit `455fb5c` split the sentence and inserted a period,
leaving `construction merely by starting.` on its own line. The complete
paragraph is restored verbatim; no new claim was introduced.

### 3.8 macOS static test

`resolve_child_directory_retains_canonical_path` now compares against
`fs::canonicalize(tmp.path()).join("subdir")`; on macOS the temp path is
`/var/...` while the descriptor-resolved path is `/private/var/...`. The
product is unchanged. `cargo test -p eggserve-static` on macOS:
**373 passed, 0 failed, 2 ignored** (previously 372/1/2).

### 3.9 Manifest provenance

`docs/release-contract.md` gains a "Release manifest provenance" subsection:
the aggregated SHA-256 `MANIFEST` is integrity-comparison data produced by the
same workflow that uploads the wheels, not independently signed provenance.
No attestation infrastructure was added.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p eggserve-bin --lib` | PASS — 75 (includes the new `--public` test) |
| `cargo test -p eggserve-static` (macOS) | PASS — 373 passed, 0 failed, 2 ignored |
| `cargo tree -p eggserve-core -e features --no-default-features` signal filter | empty |
| `cargo tree -p eggserve-bin -e features` signal filter | present |
| `git check-ignore --no-index` matrix | as tabulated |
| `git ls-files '*.pem' '*.key' '*.p12' '*.pfx' .env '.env.*'` | two tracked fixtures only |
| registry digest resolution | three digests, recorded |
| `python3 -c "yaml.safe_load(release.yml)"` | PASS |
| unpinned-`uses:` grep | empty |
| `cargo audit` with `.cargo/audit.toml` | PASS; unknown-key probe exits 1 |
| `python3 scripts/check-crate-topology.py [--self-test]` | PASS — 89 self-test checks |
| `python3 scripts/verify-conformance-matrix.py` | PASS — 51 entries, 55 app scenarios, 17 H3 |
| `python3 scripts/check-python-release-metadata.py` | PASS |
| `cargo fmt --all -- --check` | PASS |

## 5. Invariant review

| Invariant | Result |
|---|---|
| `--public` remains fail-safe; flag alone binds loopback | Held; pinned by test |
| `deny.toml` policy unchanged | Held; no diff |
| No runtime behavior change beyond the feature removal/test fix | Held |
| Release inputs pinned, not guessed | Held; digests resolved, no invented value |
| Documentation claims are executable-source-backed | Held; parser/feature/config read before editing |
| No secrets added | Held; only ignore patterns |
| No workflow trigger changes | Held; no `pull_request_target`/`workflow_run` introduced |

## 6. Compatibility and migration

- `--public`: docs only.
- `tokio/signal` removal is a dependency-graph change in an excluded feature
  passthrough, not an EggServe feature removal. `eggserve-bin` still enables it.
- `.cargo/audit.toml` replaces a file that cargo-audit never read; maintainers
  see one effective policy per tool.
- Container digests and the action inventory affect release inputs only.
- The macOS fix is test-only.

## 7. Documentation and operations

`docs/cli.md`, `docs/deployment.md`, `docs/security-policy.md`,
`docs/security-review.md`, `docs/release-contract.md`, `docs/dependency-policy.md`,
`SECURITY.md`, `docs/action-pinning.md`, `docs/threat-model.md`,
`.gitignore`, `CHANGELOG.md`, and `examples/README.md`.

## 8. Residual risk

- The MANIFEST remains unsigned; a consumer needing independent provenance must
  wait for a separate attestation milestone.
- The macOS test runs in hosted macOS compile lanes but the broad static suite
  still runs on Linux CI; the local macOS run is recorded here.
- `.key` ignores could hide a future fixture outside the negated directory; the
  negation is intentionally scoped and must be extended with any new fixture.

## 9. Roadmap disposition

`plans/subsystems/cli-ops-observability-roadmap.md` Milestone 323 is closed.

## 10. Registry updates

`plans/registry.md` row 323 moves to `closed` with a pointer to this record;
the CLI/ops roadmap milestone table and status rows are updated to match.
