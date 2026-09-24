# Plan 279 — Forward-proxy request-target seam publication and downstream closure

## Status

**CLOSED through Plan 286 registry publication and exact-artifact consumer proof.**

Plan 277 publication and Plan 279 exact-artifact closure were consolidated into Plan 286. EggServe’s publication blocker for EggReplay M013B is cleared; downstream integration remains outside this repository.

## Purpose

Publish the Plan-278 opt-in absolute-form H1 service-dispatch seam as the next
compatible 0.2.x Rust patch and prove it from clean registry-only consumers.

This plan was the downstream unblock gate for EggReplay M013B. Its exact-artifact
proof completed under Plan 286; the EggServe publication blocker is cleared.
No downstream repository integration or qualification is claimed.

## Publication baseline

Historical planning assumed a synchronized 0.2.x patch after Plan 277. Per
maintainer direction, Plan 277 publication and this plan’s registry proof were
consolidated into Plan 286. Plan 285 then measured source-incompatible API
changes, so Plan 286 selected the compatible 0.3.0 line for the affected public
crates and the next unused CLI patch `eggserve-bin 0.2.1`. Published exact
artifacts and their registry evidence are recorded in
`release/plan-286-embedding-contract-publication-closure.md`.

## Track A — Derive the minimal publish set

Use source/API/dependency evidence after Plan 278.

Expected changed authority:

- `eggserve-primitives` — new canonical target-form/absolute metadata API;
- `eggserve-server` — opt-in runtime mode and absolute-form projection.

`eggserve-core` is published only if its packaged source/API/dependency
requirements changed in a way that requires a new registry artifact. Do not
republish unrelated crates solely for synchronized workspace aesthetics.

Because server code will use the new primitives API, its registry dependency
must require a primitives release that actually contains that API. Do not leave
a broad `eggserve-primitives = "0.2.0"` requirement if it permits resolving an
older artifact that cannot compile the published server source.

Derive and record publication order. Expected:

```text
eggserve-primitives
  -> eggserve-server
  -> eggserve-core (only if required)
```

## Track B — Package qualification before publication

Run package dry-runs/staged local-registry checks using the exact candidate.

At minimum:

```bash
bash scripts/verify-cargo-packages.sh --mode all
cargo publish -p eggserve-primitives --locked --dry-run
cargo publish -p eggserve-server --locked --dry-run
```

Run core dry-run if it is in the changed publish set.

Inspect archive contents and Cargo metadata. No path/git/patch dependency may
be required by the external proof.

Run supply-chain/security checks before manual publication.

## Track C — Registry-only default-regression consumer

Create a fresh consumer outside the repository using only crates.io.

It must use the newly published `eggserve-server` and demonstrate:

1. default `RuntimeConfig` accepts origin-form;
2. default runtime rejects an H1 absolute-form request before service dispatch;
3. CONNECT authority-form remains the existing tunnel candidate behavior;
4. no `eggserve-core`, static, Tower, proxy-routing, or URL parsing dependency
   is needed.

This proves the patch did not silently widen the default server.

## Track D — Registry-only forward-proxy-shaped consumer

Create a second clean consumer outside the repository. It is generic and must
not depend on EggReplay.

Use only the direct crates necessary to:

- start `eggserve-server` with the new opt-in request-target mode;
- install a custom `Service`;
- receive raw H1 requests from a scripted local client.

Required proof:

```text
GET http://example.test:8080/a?b=1 HTTP/1.1
Host: example.test:8080
```

reaches the service with canonical metadata proving:

- absolute target form;
- scheme `http`;
- authority `example.test:8080`;
- path `/a`;
- query `b=1`.

The fixture also sends:

- ordinary origin-form under the same opt-in mode;
- Host/URI authority mismatch, which must be rejected before service;
- over-limit absolute target, which must return 414;
- CONNECT authority-form, which must remain the tunnel path.

No public Internet and no outbound forwarding are required. The consumer only
proves inbound service dispatch.

## Track E — EggReplay-shaped compatibility proof

Use a third minimal consumer or extend Track D with the exact properties
EggReplay M013B needs, while remaining generic:

- loopback listener;
- caller-owned `RuntimeConfig`;
- `OriginOrAbsolute` (or final equivalent) explicit opt-in;
- duplicate-preserving canonical headers;
- body policy selection;
- streaming request body reaches the service;
- request trailers remain intact;
- connection shutdown remains bounded;
- target form/scheme/authority can be read without Hyper types.

This proof must not import EggReplay code. It verifies the public substrate
rather than coupling repositories.

## Track F — Publication and visibility

Publish manually in dependency order.

After each publish:

- wait for crates.io sparse-index visibility;
- record publication timestamp;
- record package checksum from the registry/index;
- run `cargo info`/fresh resolution;
- do not proceed to a dependent package until the prerequisite resolves.

If any publish fails or the registry archive differs from the qualified
candidate, stop and do not claim downstream unblock.

## Track G — Exact published-artifact qualification

Re-run Tracks C–E against exact crates.io versions with:

- no workspace path;
- no git dependency;
- no `[patch.crates-io]`;
- a fresh Cargo home/lockfile where practical.

Record:

- `cargo tree -e no-dev`;
- `cargo metadata --format-version 1`;
- executable test output;
- selected versions/checksums.

Confirm the direct consumer still has no accidental core/static dependency.

## Track H — Repository evidence and downstream handoff

Create:

`release/plan-279-forward-proxy-seam-publication-closure.md`

Record:

- Plan-278 implementation SHA;
- hosted CI run;
- exact package versions;
- publication timestamps/checksums;
- default-regression consumer result;
- forward-proxy-shaped consumer result;
- EggReplay-shaped generic consumer result;
- dependency graph;
- known limitations.

Update `plans/ROADMAP.md` only after exact published artifacts pass.

Downstream handoff text should be precise:

> Published EggServe direct H1 runtime now provides an opt-in validated
> absolute-form service-dispatch mode while retaining origin-form-only defaults.
> Clean registry-only consumers prove the target-form/scheme/authority/path/query
> metadata and Host-coherence contract required by explicit forward-proxy
> embedders.

Do not claim EggServe itself is a forward proxy.

## Required hosted evidence

The publication candidate must pass the repository's current full CI before
publish. The evidence-record commit should also receive remote CI.

At minimum preserve:

- Rust stable;
- Rust 1.89;
- Linux/macOS/Windows;
- direct-server feature lanes from Plans 276–277;
- Python/package checks;
- supply-chain checks.

## Acceptance criteria

- [x] Plan 277 standalone publication was explicitly deferred; its candidate API was consolidated into Plan 286.
- [x] Plan 278 implementation/hosted CI is closed.
- [x] next available compatible patch is selected from live registry state.
- [x] minimal publish set is derived rather than assumed.
- [x] server cannot resolve against a primitives version lacking the new API.
- [x] dry-runs and staged local-registry qualification pass.
- [x] packages publish in dependency order.
- [x] checksums/timestamps are retained.
- [x] fresh registry-only default consumer proves origin-only default.
- [x] fresh registry-only opt-in consumer proves absolute-form service dispatch.
- [x] Host mismatch and target limit fail closed.
- [x] CONNECT semantics remain unchanged.
- [x] streaming body/trailer/service behavior still composes in the opt-in mode.
- [x] no path/git/patch dependency is used in final consumer proof.
- [x] no public Internet is required by runtime tests.
- [x] release closure record and roadmap status are updated.
- [x] downstream EggReplay is not marked unblocked until all above evidence
      exists.

## Non-goals

- No EggReplay source changes.
- No EggServe forward-proxy implementation.
- No CA/MITM work.
- No H2/H3 support promotion.
- No Python feature exposure.
- No automatic downstream dependency bump.
