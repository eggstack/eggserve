# CLI, Ops, and Observability Milestone 319 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/319-static-0-5-1-patch-release-and-doc-sweep.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `4a38141` (Plans 317 and 318 landed; the
`eggserve-static` version bump and the documentation sweep are in the same
commit)

Implementation commits or pull requests:

- Commit `4a38141` — the `eggserve-static` version bump, the current-line
  documentation sweep, and the six corrected doc comments.
- Commit `<PUBLICATION-COMMIT>` — the closure record, the registry rows, and
  the roadmap updates. The publication itself is a registry action, not a
  repository commit: `eggserve-static 0.5.1` was published from
  `4a38141`, whose CI run `39927249552` was green.

## 1. Executive finding

The published `eggserve-static 0.5.0` still carried the whole NT
directory-buffer defect batch that Plan 316 had already corrected in the
repository: a hand-rolled FFI struct with every field offset wrong, a
file-identity read that returned the low half of `CreationTime` instead of
the stable `FileId`, an undersized record buffer, and a zero entry budget
that returned one entry instead of none. A consumer on `0.5.0` had none of
the fixes.

This milestone published `eggserve-static 0.5.1` with them, and corrected
the documentation that described the superseded line as current.

Two documentation defects of a different kind were found and fixed in the
same milestone, because they were found by auditing Plan 313's remediation
rather than by reading the code:

1. **Six doc comments were factually wrong.** Plan 313 added 539 doc
   comments to satisfy the `missing_docs` lint. The lint only requires that
   a comment *exists*; it says nothing about whether the comment is true. An
   independent audit found six that were not — including one that published
   a false trailing-space normalization for the Windows reserved-name check,
   and one that published false response-header semantics for the CLI. All
   six are corrected.
2. **The normative release contract described the wrong line.**
   `docs/release-contract.md` — the normative reference for what EggServe
   ships — still named the superseded `0.4.0` line's per-crate versions and
   called the current tree the `0.4.x` line. Five `architecture/` deep dives
   did the same. Plan 314 swept the *pending-publication* statements and
   missed the *current-version* strings.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The current-line version statements are correct | `docs/release-contract.md`, `architecture/{eggserve-h3,eggserve-primitives,eggserve-server,eggserve-bin,eggserve-python,overview}.md` | pass |
| `eggserve-static` is bumped | `crates/eggserve-static/Cargo.toml`: `0.5.0` → `0.5.1` | pass |
| The dependent crates' requirements are untouched | `eggserve-core` and `eggserve-bin` keep `eggserve-static = "0.5.0"` — a caret, which accepts `0.5.1` | pass |
| The lockfiles move only where the version moved | All three lockfiles record `eggserve-static 0.5.1`; nothing else changed | pass |
| The package dry-run passes | `bash scripts/verify-cargo-packages.sh --mode all` → "Layered crates passed local-registry package verification" | pass |
| The publish dry-run passes | `cargo publish -p eggserve-static --locked --dry-run` → packaged 23 files, verified, compiled against the registry `eggserve-primitives 0.2.3` and `eggserve-server 0.5.0` | pass |
| The crate is published | crates.io `eggserve-static 0.5.1` | published `2026-10-23T09:08:28.400497Z`, checksum `655225bc857178bf322cdd7e25beea912e90cc1c55623644f9d1a9caea0a9651` — see §13 |
| The publication is proven against the registry | A registry-only consumer resolves and installs `0.5.1` | resolved `0.5.1`; "zero budget -> 0 entries, budget 1 -> [hello.txt]" — see §13 |
| The wrong doc comments are corrected | Six corrections, listed in §3 | pass |

## 3. Production implementation evidence

### The six corrected doc comments

Each was confirmed by reading the implementation it describes, not by
inspection of the prose.

| # | Location | The doc claimed | The code does |
|---|---|---|---|
| 1 | `crates/eggserve-static/src/path/platform.rs` — `is_windows_reserved_name` | "with trailing dots **and spaces** stripped the way Windows strips them during name resolution" | `strip_trailing_dots` strips trailing **dots only**; `"CON "` is not a reserved name by this function. (A trailing space is rejected separately by `check_component`, so the compound path was safe — but the published doc of the public function was false.) |
| 2 | `crates/eggserve-server/src/ops/counters.rs` — `dropped_log_events` | "Events that could not be delivered, either because a sink panicked **or because the owning sink filtered them out**" | The severity-filtered branch (`Ok(false) => {}`) increments nothing. Only a panicking sink — including a panicking `enabled()` probe — is counted. |
| 3 | `crates/eggserve-bin/src/args.rs` — `extra_response_headers` | "Extra headers added to final `200` responses" | Operator headers are applied to every file response (`200`/`206`/`304`/`412`/`416`) **and** to runtime error responses (`403`/`404`/`405`), so a CORS or CSP header survives an error. A reader would have concluded the opposite. |
| 4 | `crates/eggserve-server/src/ops/events.rs` — `sanitize_text_field` / `sanitize_path` | "truncated to 512 characters" / "to 127 characters" | The truncation appends a `…` sentinel, so the maximum is **513** and **128**. The file's own `truncate` doc already said "the result is one character longer than `max_len`" — the two doc blocks contradicted each other. |
| 5 | `crates/eggserve-static/src/secure_root.rs` — `ResolvedFile` | A second doc block: "This type cannot be constructed directly by external callers; it is only created through the resolver's path confinement pipeline." | The struct has a **public** `from_parts` constructor behind the `python-bindings-internal` feature, which does not re-verify provenance. The added block contradicted the pre-existing block on the same struct, and rustdoc concatenates the two. |
| 6 | `crates/eggserve-static/src/path/components.rs` — `validate_components` | Rejects "embedded separators" unconditionally | The `\` rejection is **policy-gated** (`policy.reject_backslash`); with the flag off, `a\b` is admitted. |

### The current-line documentation sweep

| Surface | Before | After |
|---|---|---|
| `docs/release-contract.md` | `server`/`static`/`h3`/`core 0.4.0`, `primitives 0.2.2`, `bin 0.2.2`, wheel `0.2.4`; "the current `main` tree is on the `0.4.x` release line" | `server`/`h3`/`core 0.5.0`, `static 0.5.1`, `primitives 0.2.3`, `bin 0.2.3`, `eggnet-tls 0.2.5`, wheel `0.2.5`; "the current `main` tree is on the `0.5.x` release line" |
| `architecture/eggserve-h3.md` | `eggserve-h3 0.4.0` | `eggserve-h3 0.5.0` |
| `architecture/eggserve-primitives.md` | `Crate version: 0.2.2` | `Crate version: 0.2.3` (the `0.5.0` line via Plan 305) |
| `architecture/eggserve-server.md` | `eggserve-server 0.4.0` | `eggserve-server 0.5.0` |
| `architecture/eggserve-bin.md` | `eggserve-bin 0.2.2`; "Published alongside the `0.4.0` leaves via Plan 300" | `eggserve-bin 0.2.3`; "Published alongside the `0.5.0` leaves via Plan 305" |
| `architecture/eggserve-python.md` | "The Python distribution is at `0.2.4` (the Rust side tracks the `0.4.0` leaves)" | "`0.2.5` (the Rust side tracks the `0.5.0` leaves)" |
| `architecture/overview.md` | `eggserve-server` … (`0.4.0`) | (`0.5.0`) |

Historical statements were left alone: "Plan 300 moved the line to
`server`/`static`/`h3`/`core 0.4.0`" is history and is correct as history.

### The version bump

`crates/eggserve-static/Cargo.toml`: `0.5.0` → `0.5.1`.

The dependent crates' requirements were deliberately **not** touched.
`eggserve-core` and `eggserve-bin` declare `eggserve-static = "0.5.0"`,
which is a caret requirement — `>=0.5.0, <0.6.0` — and already accepts
`0.5.1`. Bumping the requirement to `"0.5.1"` would narrow the accepted range
for no benefit and would force a coordinated republish of crates that have
no reason to move.

All three lockfiles record `eggserve-static 0.5.1`; no other entry moved.

## 4. Verification executed

| Check | Command | Result |
|---|---|---|
| Layered package verification | `bash scripts/verify-cargo-packages.sh --mode all` | "Layered crates passed local-registry package verification"; the layered build resolves `eggserve-static v0.5.1` from the synthetic local registry |
| Publish dry-run | `cargo publish -p eggserve-static --locked --dry-run` | packaged 23 files (480.5 KiB, 95.6 KiB compressed); verified; compiled against the registry `eggserve-primitives 0.2.3` and `eggserve-server 0.5.0` |
| Workspace clippy | `cargo clippy --workspace --lib --bins --tests -- -D warnings` | clean |
| Workspace tests | `cargo test --workspace` | exit 0 |
| Format | `cargo fmt --all -- --check` | clean |
| Topology gate | `python3 scripts/check-crate-topology.py` | exit 0 |
| Python release metadata | `python3 scripts/check-python-release-metadata.py` | pass (wheel 0.2.5, unchanged) |
| Release workflow structure | `python3 scripts/check-release-workflow.py` | pass |
| Hosted CI | CI run `39927249552` on `main` (`4a38141`) | all five jobs green |
| Publication | `cargo publish -p eggserve-static --locked` | "Uploaded eggserve-static v0.5.1 to registry `crates-io`"; published `2026-10-23T09:08:28.400497Z`, checksum `655225bc857178bf322cdd7e25beea912e90cc1c55623644f9d1a9caea0a9651` |
| Registry-only consumer | a consumer crate outside the repository resolving `eggserve-static` from the registry | resolved `0.5.1`; "registry-only consumer OK: zero budget -> 0 entries, budget 1 -> [hello.txt]" |

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| No public API change | `eggserve-static` 0.5.1 is a patch release; no item was removed, renamed, or made private | holds |
| `parse_directory_buffer`'s contract is unchanged | Its published documentation already stated that a zero budget yields an empty listing; the fix makes the code match the published documentation | holds |
| The dependency graph does not move | Only `eggserve-static`'s own version changed; no requirement, feature, or resolved version moved | holds |
| The normative documentation names the published line | §3's table | holds |
| Publication is the last step | The dry-run and the layered package verification were green before the publish | holds |

## 6. Failure and recovery review

No publication failure. The one process finding: `cargo publish --dry-run`
resolves the crate's path dependencies from the **registry**, not from the
working tree, so the dry-run is a real proof that the published artifact
builds against what consumers will actually resolve — `eggserve-primitives
0.2.3` and `eggserve-server 0.5.0`, not the local tree.

crates.io versions are immutable. If a published version needs correction, a
new version number is required; the release process records this and this
milestone does not retry a changed publish under an existing version.

## 7. Migration and compatibility review

No consumer-visible change beyond the fix itself. A consumer on
`eggserve-static 0.5.0` who needs the NT directory-buffer fixes moves to
`0.5.1`; `eggserve-core 0.5.0` and `eggserve-bin 0.2.3` accept it without a
republish, because their requirements are carets.

The changelog carries a `0.5.1` entry that states explicitly that the
`eggserve-static` crate is republished on its own line, that the rest of the
`0.5.0` crates are unchanged, and that no Python wheel release is included.

## 8. Security review

The published crate carried a defect batch; publishing the patch removes it
from the consumer's reach:

- **The file-identity read was wrong.** It returned the low half of
  `CreationTime`, not the stable `FileId`. The function's documented
  contract was "stable across renames"; the published implementation did not
  honour it.
- **The record buffer was undersized** — `80 + 256*2` bytes on a record whose
  fixed header is 104 bytes — so a name longer than 244 UTF-16 units failed
  the read with `ERROR_INSUFFICIENT_BUFFER`.
- **The zero entry budget was not honored**, so a caller asking for zero
  entries received one.

None of these is an escape from the confined root, and all three are now
closed in the published artifact.

## 9. Documentation and operations

- `CHANGELOG.md` — the `0.5.1` entry.
- `docs/release-contract.md` — the corrected current-line versions.
- `docs/migration-guide.md` — the Plan 317, 318, and 319 sections.
- `architecture/` — the corrected current-line versions.

## 10. Unresolved findings

1. **The `eggnet-tls`, `eggserve-h3`, `eggserve-core`, `eggserve-server`, and
   `eggserve-bin` republication that Plan 318's floor raise creates is still
   owed.** Their published artifacts carry the old `0.23.45` floor. That is a
   separate release line with its own plan; it is not absorbed here.
2. **The TestPyPI trusted publisher registration is still outstanding** —
   see the Plan 318 closure record §10.

## 11. Roadmap disposition

`plans/subsystems/cli-ops-observability-roadmap.md` gains **Milestone 319 —
`eggserve-static` 0.5.1 patch release and current-line sweep**, recorded as
closed in the milestone status table.

## 12. Registry updates

`plans/registry.md`: the Plan 319 row is added to the implementation-plan
table, and the next milestone number remains 320.

## 13. Publication evidence

`eggserve-static 0.5.1` was published to crates.io from the green commit
`4a381414ad1137de811e63e9d086bc58eb8d5aa6` (CI run `39927249552`, all five
jobs green).

| Field | Value |
|---|---|
| Crate | `eggserve-static` |
| Version | `0.5.1` |
| Published at | `2026-10-23T09:08:28.400497Z` |
| Checksum | `655225bc857178bf322cdd7e25beea912e90cc1c55623644f9d1a9caea0a9651` |
| Crate size | 97920 bytes (23 files; 480.5 KiB packaged, 95.6 KiB compressed) |
| MSRV | `1.89` |
| Features | `python-bindings-internal` |
| Published by | `dbowm91` |
| Yanked | no |

Declared dependencies, as the registry recorded them:

| Dependency | Requirement | Kind |
|---|---|---|
| `eggserve-primitives` | `^0.2.3` | normal |
| `eggserve-server` | `^0.5.0` | normal |
| `httpdate` | `^1` | normal |
| `phf` | `^0.11` | normal |
| `rustix` | `^1` | normal |
| `libc`, `proptest`, `tempfile` | — | dev |

`eggserve-static` declares no `rustls` dependency, which is why this release
is independent of the Plan 318 floor raise.

### Registry-only consumer proof

A consumer crate outside the repository, with no path dependency on any
EggServe crate, resolved `eggserve-static 0.5.1` from the registry and
exercised the published authority:

```
registry-only consumer OK: zero budget -> 0 entries, budget 1 -> [hello.txt]
```

The proof builds one `FILE_ID_BOTH_DIR_INFORMATION` record using the
published crate's own public layout constants and asserts both halves of the
contract the release carries:

- `parse_directory_buffer(&buffer, 0)` yields **zero** entries. The published
  `0.5.0` returned one entry for a zero budget; this is the fix the patch
  exists for, proven against the published artifact rather than the working
  tree.
- `parse_directory_buffer(&buffer, 1)` yields exactly one entry whose name
  decodes to `hello.txt` through the published layout, so the record offsets
  in the published artifact are the authoritative ones.
