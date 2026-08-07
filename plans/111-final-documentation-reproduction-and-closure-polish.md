# Plan 111 — Final Documentation Reproduction and Closure Polish

## Status

**Pending implementation.**

This is a documentation-only corrective pass against repository state:

```text
dc66811130ebdb43eb605bcaba823a2854287549
```

Plan 109 remains functionally complete. Plan 110 landed the intended documentation
cleanup in broad terms, but a post-implementation review found three residual
documentation defects:

1. `benchmarks/binary-size.md` documents a packaged-CLI hash procedure that cannot
   succeed as written because `scripts/test-python-wheel.sh` removes both the staged
   CLI and its temporary wheel directory in its `EXIT` cleanup before the following
   hash commands run;
2. filesystem documentation overstates the Windows request lifecycle by saying
   `RootGuard` clones the pinned root handle per request, while the Windows hardened
   path normally borrows the retained root handle as `ObjectAttributes.RootDirectory`
   authority and opens owned descendants relative to it; only the root-directory
   result duplicates the root handle;
3. Plan 110 declares itself complete but leaves its final acceptance checklist
   unchecked and its closure record contains imprecise implementation metadata,
   including an eight-file count that excludes the Plan 110 self-update from the
   actual nine-file commit.

No runtime, test, build, packaging, CI, release, or API defect is reopened by this
plan.

No new roadmap is authorized. This is the final documentation reconciliation pass for
Plans 109–110.

---

## Goal

Make the active documentation mechanically consistent with the verified repository
behavior and with the actual Plan 110 commit history.

The final documentation should state the following accurately.

### Filesystem authority

```text
static-service construction
    -> ServeState constructs and retains one PinnedRoot

Unix hardened request traversal
    -> RootGuard borrows PinnedRoot
    -> resolver duplicates/clones the root fd as request traversal authority
    -> descendant descriptors are opened relative to the current descriptor

Windows hardened request traversal
    -> RootGuard borrows PinnedRoot
    -> resolver uses the retained raw root handle as RootDirectory authority
    -> descendant owned handles are opened relative to that retained authority
    -> root-directory response duplicates the retained root handle when an owned
       result is required

all hardened traversal
    -> configured root pathname is not reopened per request
```

### Artifact reproduction

```text
clean build state
    -> build and copy each default/TLS artifact immediately to a unique capture path
    -> preserve the default dist capture before any TLS overwrite
    -> create a staged package tree and wheel in locations that remain available
       long enough to hash
    -> extract the bundled CLI from that wheel
    -> compare SHA-256 of:
         default dist capture
         staged bundled CLI
         wheel-extracted bundled CLI
```

The documented procedure must not depend on files that the supported verification
script deletes before the next command runs.

### Plan 110 closure

```text
Plan 110 implementation commit
    = dc66811130ebdb43eb605bcaba823a2854287549

actual files changed by that commit
    = 9 documentation/planning files

final acceptance checklist
    = checked only for items actually satisfied

runtime status
    = unchanged; Plan 109 remains functionally closed
```

---

## Scope

This plan authorizes documentation edits only.

Expected primary files:

```text
benchmarks/binary-size.md
architecture/filesystem-confinement.md
docs/architecture.md
plans/110-documentation-closure-polish.md
plans/111-final-documentation-reproduction-and-closure-polish.md
```

Conditionally edit only if a concrete contradictory statement remains:

```text
architecture/eggserve-core.md
architecture/runtime.md
architecture/overview.md
README.md
AGENTS.md
.opencode/skills/eggserve-dev/SKILL.md
```

Do not churn conditional files solely to mention Plan 111.

---

## Non-goals

Do not:

- change Rust source;
- change Python source;
- change tests or fixtures;
- change Cargo manifests, lockfiles, features, or build profiles;
- change `scripts/test-python-wheel.sh`;
- add a new artifact-retention option to scripts;
- change release workflows;
- add CI jobs;
- rerun or replace Plan 109 artifact values absent evidence that the existing values
  are incorrect;
- reopen runtime admission, Stream semantics, static metadata, filesystem hardening,
  or Python callback work;
- redesign `PinnedRoot` or `RootGuard`;
- add a permanent benchmark framework;
- add an automated documentation checker;
- broaden EggServe beyond its current hardened HTTP/1.1 static-server and reusable
  HTTP-primitives scope;
- create another follow-up plan for minor prose preferences after this pass.

The final implementation diff for Plan 111 must contain documentation/planning files
only.

---

# Track A — Establish the exact documentation baseline

Before editing, record these evidence anchors:

| Purpose | Commit / evidence |
|---|---|
| Plan 109 functional implementation | `cea39f779b4f6b828c92ff8bd9332bd0d2d1d99d` |
| Plan 109 artifact evidence | `d273134aa7eb1583106afc00f4e24dc09e0aeb91` |
| Plan 109 hosted-CI-tested implementation tree | `49ecb712be1677a027891ad373b6951d7b916182` |
| Plan 109 documentation closure | `3b75bd621a90a94fc5d732a1afb4f36e03b255dd` |
| Plan 110 plan creation | `207ae12d3cceb7e706eec2fe00eabc15d153536d` |
| Plan 110 implementation | `dc66811130ebdb43eb605bcaba823a2854287549` |
| Hosted CI for Plan 109 verified tree | run `31035414453` |

Plan 111 must not change what these commits mean.

Run targeted searches before editing:

```sh
rg -n "RootGuard|PinnedRoot|clone.*root|root.*clone|handle.*per request|per-request.*handle" \
  architecture docs README.md AGENTS.md .opencode plans

rg -n "test-python-wheel|staged binary|wheel-extracted|artifact_dir|sha256sum|SHA-256" \
  benchmarks docs plans

rg -n "Plan 110|dc668111|8 files|eight files|9 files|nine files|Final acceptance checklist" \
  plans AGENTS.md .opencode architecture docs
```

Classify each match as:

- correct current-state wording;
- correct historical wording;
- inaccurate current-state wording;
- stale implementation instruction;
- harmless example inside a plan;
- acceptance-state metadata needing reconciliation.

Do not rewrite historical descriptions merely because terminology has since improved.

### Acceptance criteria

- baseline SHA is `dc66811130ebdb43eb605bcaba823a2854287549`;
- all three known residual defects are located before editing;
- no runtime issue is reclassified as active;
- no unrelated documentation cleanup enters scope.

---

# Track B — Make artifact reproduction executable as written

## B1 — Preserve the supported verification-script behavior

The current `scripts/test-python-wheel.sh` intentionally:

- builds the default non-TLS `dist` CLI;
- stages that CLI into the Python package tree;
- creates its wheel in a temporary directory;
- installs and tests the wheel;
- removes the temporary wheel directory on exit;
- removes the staged CLI it created on exit.

That cleanup behavior is valid and must not be changed for this documentation task.

The documentation must therefore stop instructing readers to run the script and then
hash files that the script has already removed.

## B2 — Replace the impossible post-script hash sequence

In `benchmarks/binary-size.md`, remove or rewrite the sequence equivalent to:

```sh
PYTHON=python3.14 bash scripts/test-python-wheel.sh
sha256sum crates/eggserve-python/python/eggserve/bin/eggserve
# then find a wheel in repository dist/
```

Do not document a path that the script does not preserve.

## B3 — Preferred reproduction procedure

Prefer a documentation-only manual capture procedure that mirrors packaging semantics
without modifying the supported verification script.

A good procedure is:

```sh
set -euo pipefail
artifact_dir="$(mktemp -d)"
wheel_dir="$(mktemp -d)"
stage_dir="crates/eggserve-python/python/eggserve/bin"

rm -rf target/release target/dist
rm -rf crates/eggserve-python/target
rm -rf "$stage_dir"

cargo build --release --locked -p eggserve-bin
cp target/release/eggserve "$artifact_dir/eggserve-default-release"

cargo build --profile dist --locked -p eggserve-bin
cp target/dist/eggserve "$artifact_dir/eggserve-default-dist"

cargo build --release --locked -p eggserve-bin --features tls
cp target/release/eggserve "$artifact_dir/eggserve-tls-release"

cargo build --profile dist --locked -p eggserve-bin --features tls
cp target/dist/eggserve "$artifact_dir/eggserve-tls-dist"
```

Then explicitly rebuild the default non-TLS `dist` CLI because the TLS build overwrote
the shared target filename:

```sh
cargo build --profile dist --locked -p eggserve-bin
mkdir -p "$stage_dir"
cp target/dist/eggserve "$stage_dir/eggserve"
chmod +x "$stage_dir/eggserve"
```

Build the wheel directly into the persistent temporary wheel directory using the same
packaging profile:

```sh
(
  cd crates/eggserve-python
  python3.14 -m maturin build \
    --profile dist \
    --interpreter python3.14 \
    -o "$wheel_dir"
)
```

If shell-relative path resolution makes the `wheel_dir` variable ambiguous from the
subdirectory, document an absolute path:

```sh
wheel_dir="$(cd "$(mktemp -d)" && pwd)"
```

Extract the bundled CLI to a persistent capture path:

```sh
python3.14 - "$wheel_dir" "$artifact_dir/eggserve-wheel-extracted" <<'PY'
import pathlib
import sys
import zipfile

wheel_dir = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
wheel = next(wheel_dir.glob("eggserve-*.whl"))
with zipfile.ZipFile(wheel) as zf:
    members = [
        name for name in zf.namelist()
        if name.endswith("/eggserve") or name.endswith("/eggserve.exe")
    ]
    if len(members) != 1:
        raise SystemExit(f"expected one bundled CLI, found {members!r}")
    out.write_bytes(zf.read(members[0]))
PY
```

Then compare the actual persistent files:

```sh
sha256sum \
  "$artifact_dir/eggserve-default-dist" \
  "$stage_dir/eggserve" \
  "$artifact_dir/eggserve-wheel-extracted"
```

All three hashes must match for the same platform and package configuration.

The documentation may use a platform-variable binary name where appropriate:

```sh
case "$(uname -s)" in
  *MINGW*|*MSYS*|*CYGWIN*) bin_name=eggserve.exe ;;
  *) bin_name=eggserve ;;
esac
```

The recorded Plan 109 measurement snapshot is Linux
`x86_64-unknown-linux-gnu`; do not pretend the exact Linux commands are portable to
PowerShell without adaptation.

## B4 — Keep verification and measurement concepts separate

Document clearly:

- `scripts/test-python-wheel.sh` remains the supported installed-wheel verification
  harness;
- the manual capture recipe exists specifically to preserve artifacts long enough for
  reproducibility and hash comparison;
- running the manual capture recipe does not replace the installed-wheel verification
  harness;
- no artifact-retention behavior is being added to production scripts.

## B5 — Preserve recorded evidence

Do not change existing recorded sizes or SHA-256 values unless a fresh reproduction
proves they are wrong.

If no fresh measurement is performed during Plan 111, say so explicitly.

### Acceptance criteria

- every path referenced after a command still exists at that point in the documented
  procedure;
- the procedure does not rely on `test-python-wheel.sh` retaining temporary artifacts;
- the default `dist` CLI is rebuilt/restaged after the TLS build overwrites the shared
  target path;
- wheel output is placed in a known persistent temporary directory;
- the bundled CLI is extracted before cleanup;
- SHA-256 compares the default-dist capture, staged bundled CLI, and wheel-extracted
  bundled CLI;
- the supported installed-wheel verification script remains unchanged;
- existing artifact values remain unchanged absent new evidence.

### Rejection conditions

Reject this track if:

- it modifies `scripts/test-python-wheel.sh` solely for documentation convenience;
- it hashes a staged file after a script has deleted it;
- it searches repository `dist/` for a wheel that the script never writes there;
- it compares the TLS binary to the bundled non-TLS binary;
- it allows a stale TLS-overwritten `target/dist/eggserve` to stand in for the default
  capture;
- it changes historical measurement values without a reproducible basis.

---

# Track C — Correct Windows `RootGuard` lifecycle wording

## C1 — State the common ownership model accurately

The common statement should be:

```text
RootGuard is request-scoped and borrows the long-lived PinnedRoot.
```

Do not claim that `RootGuard` itself always duplicates the root descriptor or handle.
The actual duplication/open behavior belongs to the platform resolver.

## C2 — Unix wording

For hardened Unix traversal, documentation may state:

- `RootGuard` borrows `PinnedRoot`;
- `resolve_fd_relative()` starts from the retained root fd;
- the resolver duplicates/clones that root fd into request traversal state;
- subsequent components are opened descriptor-relative;
- owned request descriptors are released as their scoped values are dropped;
- the configured pathname is not reopened.

This wording matches the current Unix resolver behavior.

## C3 — Windows wording

For hardened Windows traversal, documentation must state:

- `RootGuard` borrows `PinnedRoot`;
- the resolver receives the retained raw root handle;
- for ordinary descendant traversal, that retained root handle is used as
  `ObjectAttributes.RootDirectory` authority for the first relative open;
- descendant files/directories are represented by owned handles opened relative to the
  current parent;
- the retained root handle is not wrapped as an owned child handle during ordinary
  traversal;
- when the requested resource is the root directory itself, the resolver duplicates the
  retained root handle so the returned `ResolvedDirectory` owns its handle;
- no configured-root pathname reopen occurs under hardened traversal.

Preferred compact wording:

```text
One PinnedRoot is retained per static service. One RootGuard is created per static
request. RootGuard borrows the pinned filesystem authority. Unix resolution duplicates
the retained root descriptor for traversal; Windows resolution normally uses the
retained root handle directly as handle-relative RootDirectory authority and opens
owned descendants from it, duplicating the root handle only when an owned root-directory
result is required.
```

## C4 — Files to reconcile

At minimum inspect:

```text
architecture/filesystem-confinement.md
docs/architecture.md
```

Also search active docs for phrases such as:

```text
clones the pinned root handle per request
clones its descriptor or handle per request
RootGuard duplicates the root
request-scoped root handle clone
```

Correct only actual current-state inaccuracies.

## C5 — Preserve security claims without broadening them

Do not weaken the intended hardened guarantees:

- Unix remains descriptor-relative under symlink-denied policy;
- Windows remains handle-relative under symlink-denied policy;
- neither hardened path reopens the configured root pathname per request;
- fallback/follow-symlink modes retain their existing weaker qualification.

Do not add a claim that Windows duplicates the root handle for every request merely to
make the Unix and Windows descriptions symmetrical.

### Acceptance criteria

- no active doc says Windows clones/duplicates the pinned root handle on every request;
- active docs still state one request-scoped `RootGuard` per static request;
- Unix descriptor duplication is attributed to the Unix resolver, not to the abstract
  `RootGuard` type itself;
- Windows retained-root-handle authority is described correctly;
- root-directory handle duplication on Windows is documented only where relevant;
- no pathname reopen is implied in hardened mode;
- no platform security guarantee is broadened or weakened.

### Rejection conditions

Reject this track if documentation:

- claims the Windows root handle is duplicated for all requests;
- claims Windows uses reconstructed absolute paths in hardened mode;
- implies the long-lived root handle is transferred into request-owned state;
- says `RootGuard` itself owns the root authority;
- rewrites implementation code to match simplified documentation.

---

# Track D — Reconcile Plan 110 checklist and closure metadata

## D1 — Record the exact implementation commit

In the Plan 110 closure record, replace vague wording such as:

```text
Implementation commit: documentation-only diff against main
```

with:

```text
Implementation commit:
`dc66811130ebdb43eb605bcaba823a2854287549`
```

If Plan 111 later modifies Plan 110, distinguish:

- Plan 110 implementation commit: `dc668111...`;
- Plan 111 documentation-correction commit: the new final SHA.

Do not rewrite the chronology as if Plan 111 were part of the original Plan 110
implementation.

## D2 — Correct the changed-file count

The actual commit `dc668111...` changed nine files:

```text
.opencode/skills/eggserve-dev/SKILL.md
AGENTS.md
architecture/eggserve-core.md
architecture/filesystem-confinement.md
architecture/overview.md
benchmarks/binary-size.md
docs/architecture.md
plans/109-final-admission-and-wire-verification-corrective-pass.md
plans/110-documentation-closure-polish.md
```

Plan 110's closure record currently says `git diff --name-only — 8 files` because the
self-update was apparently omitted from the reported count.

Correct the record to distinguish:

- eight pre-existing documentation files edited to implement the plan;
- plus `plans/110-documentation-closure-polish.md` updated to record closure;
- nine files in the final commit.

Preferred wording:

```text
Final commit path count: 9 documentation/planning files. Eight pre-existing
repository documents were corrected, and Plan 110 itself was updated with status and
closure evidence.
```

Do not fabricate a historical command output that was not actually captured. If the
original `git diff --name-only` command was run before the self-update, say so.

## D3 — Reconcile the final acceptance checklist

Plan 110 currently has a completed status and a closure record while the final
acceptance checklist remains entirely unchecked.

Update each checkbox based on actual evidence.

For criteria satisfied by `dc668111...`, check them.

For criteria discovered to have been imperfect after closure:

- do not backdate a false claim that they were perfect in the original commit;
- either leave them qualified and point to Plan 111, or update the checklist after
  Plan 111 correction with an explicit note that Plan 111 supplied final polish.

Recommended final approach after Plan 111 implementation:

```text
Plan 110: COMPLETE — documentation correction implemented in dc668111..., with final
reproduction/lifecycle/closure metadata polish completed by Plan 111.
```

Then mark the checklist satisfied in the current tree, not necessarily by the original
Plan 110 commit alone.

## D4 — Add a concise supersession note

At the Plan 110 closure record, add a short note such as:

```text
Post-closure review identified three documentation-only residuals: the artifact
reproduction commands depended on temporary files removed by the wheel test harness,
Windows RootGuard wording overstated root-handle duplication, and closure bookkeeping
needed reconciliation. Plan 111 corrects those documentation issues without reopening
Plan 109 or changing runtime behavior.
```

Keep this note concise. Do not reintroduce a long active handoff section into a closed
plan.

## D5 — Close Plan 111 cleanly

When Plan 111 is implemented, its closure record must contain:

- exact implementation commit SHA;
- exact changed-path list or count;
- confirmation that only documentation/planning files changed;
- exact artifact-reproduction wording correction performed;
- exact Windows lifecycle wording correction performed;
- Plan 110 checklist/metadata reconciliation performed;
- `git diff --check` result;
- targeted stale-phrase search results;
- explicit statement that no source, tests, scripts, manifests, workflows, or runtime
  behavior changed.

Do not require hosted CI for a documentation-only correction unless repository policy
runs it automatically and the result is convenient to record. Plan 109's existing
hosted CI evidence remains the runtime verification authority.

### Acceptance criteria

- Plan 110 identifies `dc66811130ebdb43eb605bcaba823a2854287549` exactly;
- Plan 110's final commit file count is reconciled to nine;
- the distinction between eight pre-existing files and the Plan 110 self-update is
  explicit if useful;
- Plan 110's acceptance checklist is no longer all unchecked while status says
  complete;
- any criterion completed only by Plan 111 is transparently attributed to Plan 111;
- Plan 110 remains a historical documentation pass, not an active runtime plan;
- Plan 111 receives its own concise closure record.

---

# Track E — Focused verification

This is a documentation-only pass. Verification should remain proportional.

## E1 — Diff scope

Before closing:

```sh
git diff --name-only <plan-111-base>...HEAD
```

Every changed path must be documentation/planning material.

Reject the implementation if any of the following change:

```text
*.rs
*.py
Cargo.toml
Cargo.lock
.github/workflows/*
scripts/*
conformance/*
fuzz/*
```

unless the change is an accidental formatting artifact that is reverted before
closure.

## E2 — Documentation hygiene

Run:

```sh
git diff --check
```

Optional, low-cost repository hygiene:

```sh
cargo fmt --all -- --check
```

Do not rerun the full workspace test matrix solely to close this documentation plan
unless repository practice makes it effectively free. Functional correctness remains
anchored to Plan 109's verified implementation tree.

## E3 — Targeted artifact-procedure review

Read the documented commands in order and verify mechanically:

1. all variables are defined before use;
2. all output paths survive until the hash comparison;
3. default and TLS artifacts are captured before overwrite;
4. default `dist` is rebuilt before staging after the TLS build;
5. wheel output directory is stable and discoverable;
6. extraction identifies exactly one bundled CLI member;
7. hash commands point to real persistent paths;
8. cleanup, if documented, occurs only after measurement.

If practical, execute the artifact procedure locally. If not executed, do not claim it
was executed; state that the procedure was reviewed against script behavior.

## E4 — Targeted stale-phrase checks

Run searches equivalent to:

```sh
rg -n "clones the pinned root.*handle.*per request|descriptor.*or handle.*per request" \
  architecture docs README.md AGENTS.md .opencode

rg -n "PYTHON=.*test-python-wheel.*\n.*sha256sum|dist/eggserve-.*\.whl" \
  benchmarks docs plans

rg -n "git diff --name-only.*8 files|Implementation commit: documentation-only diff" \
  plans/110-documentation-closure-polish.md
```

Adapt multiline searches as necessary.

Expected result:

- no active Windows claim that the root handle is cloned every request;
- no active artifact recipe that hashes wheel-script temporary outputs after script
  exit;
- no stale eight-file final-commit claim in Plan 110;
- no vague implementation-commit placeholder in the Plan 110 closure record.

## E5 — Check current code only as an authority reference

Do not modify code. Use it only to confirm documentation wording:

```text
crates/eggserve-core/src/fs/mod.rs
crates/eggserve-core/src/fs/unix.rs
crates/eggserve-core/src/fs/windows.rs
scripts/test-python-wheel.sh
```

Key code facts to preserve:

- `RootGuard<'a>` stores `&'a PinnedRoot`;
- Unix hardened resolution starts by cloning the root fd;
- Windows hardened resolution takes a raw root handle and opens descendants relative
  to it;
- Windows root-directory resolution duplicates the raw root handle for owned return;
- wheel-test cleanup removes its staged binary and temporary wheel directory.

### Acceptance criteria

- changed paths are documentation-only;
- `git diff --check` passes;
- artifact commands are internally executable in sequence;
- Windows lifecycle text matches code;
- Plan 110 metadata matches commit history;
- no runtime verification claim is reassigned to a documentation-only commit.

---

# Final acceptance checklist

## Artifact reproduction

- [ ] documentation no longer hashes files after `test-python-wheel.sh` deletes them;
- [ ] documentation no longer assumes the verification script writes a persistent wheel
      to repository `dist/`;
- [ ] default release, default dist, TLS release, and TLS dist are captured to unique
      paths immediately after each build;
- [ ] default dist is rebuilt before package staging after TLS overwrites the shared
      target path;
- [ ] staged CLI exists during hash comparison;
- [ ] wheel exists during extraction and hash comparison;
- [ ] wheel-extracted CLI is saved to a persistent comparison path;
- [ ] default-dist capture, staged CLI, and wheel-extracted CLI are compared by SHA-256;
- [ ] `scripts/test-python-wheel.sh` remains unchanged;
- [ ] recorded Plan 109 artifact values are unchanged absent new evidence.

## Filesystem lifecycle wording

- [ ] one `PinnedRoot` per static service remains documented;
- [ ] one request-scoped `RootGuard` per static request remains documented;
- [ ] `RootGuard` is described as borrowing `PinnedRoot`;
- [ ] Unix root-fd duplication is attributed to Unix traversal;
- [ ] Windows normal descendant traversal uses retained root-handle authority directly;
- [ ] Windows root-directory result duplication is described accurately;
- [ ] no active document claims Windows duplicates the root handle every request;
- [ ] hardened traversal remains documented as avoiding configured-root pathname
      reopen.

## Plan 110 closure bookkeeping

- [ ] Plan 110 records implementation commit
      `dc66811130ebdb43eb605bcaba823a2854287549`;
- [ ] Plan 110 final commit count is reconciled to nine documentation/planning files;
- [ ] eight pre-existing corrected files versus the Plan 110 self-update are
      distinguished where useful;
- [ ] Plan 110 final acceptance checklist reflects the current corrected state;
- [ ] criteria completed by Plan 111 are attributed transparently;
- [ ] Plan 110 remains functionally/documentationally historical and closed;
- [ ] Plan 109 remains functionally closed.

## Scope and verification

- [ ] no Rust or Python source changes;
- [ ] no test changes;
- [ ] no script changes;
- [ ] no manifest or lockfile changes;
- [ ] no workflow or release changes;
- [ ] no new CI jobs or gates;
- [ ] `git diff --check` passes;
- [ ] targeted stale-phrase searches pass;
- [ ] Plan 111 receives a concise closure record with its exact implementation SHA.

---

# Rejection criteria

Plan 111 is not complete if any of the following remain true:

1. the artifact recipe invokes `test-python-wheel.sh` and then hashes its deleted staged
   or temporary wheel outputs;
2. the artifact recipe uses an overwritten TLS `target/dist/eggserve` as though it were
   the captured default binary;
3. the artifact recipe cannot locate the wheel it says to extract;
4. active docs say Windows clones the pinned root handle on every request;
5. active docs imply `RootGuard` owns rather than borrows the root authority;
6. Plan 110 still claims an eight-file final implementation commit without explaining
   its self-update;
7. Plan 110 remains complete while all final acceptance checkboxes are left unchecked;
8. Plan 110 still uses a vague implementation-commit placeholder instead of
   `dc668111...`;
9. runtime, tests, packaging scripts, manifests, workflows, or release behavior are
   changed;
10. a new broad roadmap or verification framework is introduced to solve these three
    documentation defects.

---

# Suggested implementation sequence

One documentation commit is preferred unless the repository workflow requires a
separate closure commit.

Recommended sequence:

```text
1. docs: fix artifact reproduction and Windows root-handle wording
2. docs: reconcile Plan 110 and close Plan 111
```

A single commit is acceptable if it remains easy to audit and contains documentation
only.

Do not split this into multiple implementation phases.

---

# Handoff summary

This is the final narrow documentation cleanup after the functionally complete Plan
109 runtime work and the mostly-correct Plan 110 documentation pass.

The implementer should:

1. keep `dc66811130ebdb43eb605bcaba823a2854287549` as the baseline;
2. fix `benchmarks/binary-size.md` so every documented artifact survives until its
   stated hash comparison;
3. leave `scripts/test-python-wheel.sh` unchanged;
4. correct Windows `RootGuard` wording to retained-root-handle authority rather than
   per-request root-handle duplication;
5. reconcile Plan 110's implementation SHA, nine-file final count, checklist, and
   supersession note;
6. verify a documentation-only diff;
7. close Plan 111 with exact commit metadata.

No runtime work remains authorized or required under this plan.
