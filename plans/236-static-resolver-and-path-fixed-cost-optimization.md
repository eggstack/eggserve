# Plan 236 — Static resolver and path fixed-cost optimization

## Prerequisite

Plan 234 must retain syscall/allocation evidence for hardened static requests.
The resolver changes in this plan must be independently reviewable from path
allocation changes.

## Purpose

Reduce fixed per-request work in `eggserve-static` while preserving its role
as the sole confinement/static authority and preserving the hardened
descriptor-relative security model.

## Track A — remove the non-root Unix root-FD duplicate

The current hardened Unix resolver begins non-root traversal by cloning the
pinned root `File`. On Linux this produces an avoidable descriptor-duplication
syscall and close for ordinary file requests.

Refactor traversal so the first component operates against a borrowed pinned
root descriptor and ownership begins only after an intermediate directory is
actually opened.

A simple ownership model is:

```text
current_owned: Option<File>
current = current_owned.as_ref().unwrap_or(root_fd)
```

with `statat` and `openat` both using the same current directory capability
for each component.

Requirements:

- a non-root one-component file request must not duplicate the root FD;
- nested traversal must retain ownership of opened intermediate directory FDs;
- resolving the root itself may still duplicate the root handle when an owned
  `ResolvedDirectory` capability is required;
- no raw-FD reconstruction, unsafe `from_raw_fd`, or pathname reopen;
- descriptor lifetime must remain valid across every `statat`/`openat`;
- all error mapping and denial behavior remain unchanged.

Do **not** remove the pre-open `statat(AT_SYMLINK_NOFOLLOW)` solely for
performance. It rejects symlinks and dangerous special files before open.
Do **not** remove post-open metadata/identity validation solely because a
pre-check exists.

## Track B — static path common-case fast path

Current confinement performs percent decoding and normalization through
intermediate owned strings even for an already-normalized path.

Add an internal common-case path that avoids unnecessary intermediate
allocation when the input:

- contains no `%`;
- contains no duplicate slash requiring collapse;
- contains no leading form ambiguity beyond the already-validated origin path;
- still undergoes component validation, dot/dotdot checks, dotfile policy,
  backslash policy, control/NUL checks, and platform checks.

Do not change the signatures or behavior of public
`path::decode::percent_decode`, `components::normalize_path`,
`components::split_components`, or `ConfinedPath` accessors merely to
optimize the internal orchestrated path.

For encoded/non-normal paths, either retain the current helpers or fuse decode
+ normalization in a private helper only when tests prove byte-identical
results.

## Track C — range parsing temporary cleanup

Where the static range evaluator currently materializes a temporary vector only
to distinguish single-range from multi-range input, replace it with iterator
lookahead/counting.

Preserve RFC behavior exactly, including:

- multiple-range policy;
- invalid inverted range being ignored/full response according to the existing
  Plan-168 contract;
- unsatisfiable valid ranges yielding 416;
- suffix/open-ended behavior;
- whitespace handling.

## Security/regression tests

Extend confinement tests for:

- one-component and multi-component regular files;
- symlink swap/rejection cases;
- FIFO/device/special-file rejection;
- root directory resolution;
- nested directory/index/listing paths;
- dotfiles;
- percent and double-encoded traversal;
- repeated slash normalization;
- platform-specific component rejection.

On Unix add a regression capable of proving the ordinary one-component path
does not clone the root descriptor, using syscall evidence in qualification or
a narrow test hook that is absent from production builds.

Windows behavior must remain unchanged except for shared path-processing code.

## Measurement

Against Plan 234:

- syscall counts for one-component/nested/root static requests;
- static 1 KiB c1/c16/c64;
- path-heavy microbench for normalized, encoded, and nested paths;
- HEAD/304/range cases;
- allocations per confinement operation where available.

The primary Track-A success criterion is mechanical: one unnecessary root-FD
duplication/close pair disappears from non-root hardened requests while all
security-required syscalls remain.

## Non-goals

- No descriptor/path cache.
- No weakening of `O_NOFOLLOW`, `AT_SYMLINK_NOFOLLOW`, or special-file
  defenses.
- No canonical-path fallback in the hardened profile.
- No public `ConfinedPath` representation/API change.
- No sendfile/mmap/io_uring work.

## Acceptance criteria

- [ ] Non-root hardened Unix traversal starts from the borrowed pinned root.
- [ ] Root and nested descriptor ownership is correct on all exits.
- [ ] Confinement/security regression suites pass.
- [ ] Common normalized paths avoid at least one proven temporary allocation
      when Plan 234 shows that target.
- [ ] Range parsing avoids temporary collection without semantic change.
- [ ] Same-machine syscall/allocation/performance evidence is retained.
