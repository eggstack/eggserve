# Plan 291 — Direct H1 boundary-ownership publication closure

Status: **COMPLETE**.

## Proof-bearing source and hosted CI

- Plan 290 implementation SHA: `dc39fef20dd658755ef268c4cd82916448fa3da1`.
- Hosted CI run: [36102738591](https://github.com/eggstack/eggserve/actions/runs/36102738591), conclusion **success**.
- Local release gates and the qualification summary: [Plan 290 evidence](plan-290-direct-h1-boundary-ownership-qualification.md).

## Live registry state and version decision

Before publication, crates.io reported `eggserve-server 0.3.0`,
`eggserve-core 0.3.0`, and `eggserve-primitives 0.2.1`. Plan 290 classified
the public change as additive, so the minimal compatible patch was
`eggserve-server 0.3.1`. Core's existing `^0.3.0` dependency can resolve that
patch; republishing core or any sibling was unnecessary.

Published only `eggserve-server 0.3.1` on 2026-09-25 at
`2026-09-25T06:42:02.912508Z`. crates.io reports archive SHA-256:

```text
987b5873f0273b4d02f81f8e19613c93825803c224e972c86e9e9888098f3de8
```

`cargo publish -p eggserve-server --locked` packaged and verified the crate
before upload. The package dry-run and full package verification had passed
before the proof-bearing source commit.

## Registry-only consumers

Fresh manifests and lockfiles were created under `/tmp/eggserve-plan291-*`,
outside the workspace. Their manifests contain only exact registry versions;
they use no path, git, patch, or workspace dependencies.

| Consumer | Registry requirements | Result |
| --- | --- | --- |
| Direct H1 + TLS | `eggserve-server = 0.3.1`, `eggserve-primitives = 0.2.1` | 14 downstream embedding tests passed, including default policy, parser values above former limits, external aggregate header ownership, sequential Date/Server and absence, invalid Date handling, caller-owned TCP, and caller-owned Rustls/Tokio-Rustls H1. |
| Tower/Axum | Same versions, server `tower` feature | 2 Tower qualification tests passed, including external Date/Server, duplicate application headers, and streaming response behavior. |
| Compatibility core | `eggserve-core = 0.3.0` | `cargo check` passed; resolved graph selected `eggserve-server 0.3.1` through core's existing compatible requirement. |

The direct H1 and Tower dependency trees resolve `eggserve-server 0.3.1` and
`eggserve-primitives 0.2.1`, and contain no `eggserve-core`,
`eggserve-static`, or PHF. The core graph contains `eggserve-core 0.3.0` and
`eggserve-server 0.3.1`.

Commands run against these registry-only consumers:

```text
cargo test --manifest-path /tmp/eggserve-plan291-direct/Cargo.toml
cargo test --manifest-path /tmp/eggserve-plan291-tower/Cargo.toml --features tower
cargo check --manifest-path /tmp/eggserve-plan291-core/Cargo.toml
```

## Downstream unblock

The published artifact that provides all three requested capabilities is
**`eggserve-server 0.3.1`**:

- parser `max_buf_size` and `max_headers` accept explicit values above the
  former EggServe maxima while retaining the Hyper minimum and positive count;
- direct H1 can explicitly externalize aggregate post-parse header-byte
  policy;
- successful service responses can explicitly own Date/Server metadata,
  while runtime-generated errors, response framing, and denylisted fields
  remain runtime-owned.

Secure defaults remain unchanged. `eggserve-core 0.3.0`, static/H3/bin crates,
and the Python wheel were not republished. H2/H3 support tiers and all
non-H1 transport ownership remain unchanged.
