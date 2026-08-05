# Plan 106 — Verification, CI, Documentation, and Final Closure

## Status

**Implementation complete.** Final execution and closure plan under Plan 102.

This plan begins only after Plans 103–105 are implemented. It simplifies routine verification, preserves security-critical coverage, reconciles active documentation, validates manual release artifacts, and closes the roadmap on one verified commit.

It does not authorize new product behavior.

## Goal

Close the corrective track with a verification system proportionate to a small local/LAN static server:

1. routine CI remains two fast, blocking Ubuntu jobs;
2. expensive and platform-specialist verification remains manual;
3. redundant fuzz targets are consolidated without weakening security-critical parser/path/framing coverage;
4. manual release wheel builds perform minimal installed-artifact smoke tests on each built platform;
5. release publication remains manual and outside CI;
6. active documentation describes the final runtime ownership, configuration, scope, and size accurately;
7. stale counts, commands, plan ranges, and closure claims are removed;
8. local and hosted checks pass on the same final commit;
9. one concise closure record distinguishes implemented fixes from measured non-changes.

## Governing constraints

- Keep exactly the existing routine `rust` and `python` CI jobs unless combining a step reduces work without obscuring failures.
- Do not add an OS matrix to pull-request CI.
- Do not add scheduled workflows.
- Do not add automated crates.io or PyPI publication.
- Do not add evidence aggregation, attestations, SBOM generation, release criteria engines, or generated gate registries.
- Do not add timing-sensitive soak tests to routine CI.
- Do not require cargo-fuzz, cargo-bloat, Caddy, nginx, or platform-specific adversarial tools for routine development.
- Do not delete deterministic security regression tests merely to reduce counts.
- Do not rewrite all documentation; update only active files made inaccurate by Plans 103–105.
- Do not mark the roadmap complete before the final hosted jobs are visible on the exact closure commit.

## Verification tiers

### Tier 1 — Routine development and pull-request CI

Purpose: fast feedback on common regressions.

Required contents:

```text
format
normal clippy for production/test targets
workspace tests
focused server TLS tests
installed-wheel Python build/smoke/tests
```

Target outcome: the common loop remains comprehensible and normally completes within the existing job timeouts.

### Tier 2 — Manual full verification

Purpose: pre-release or substantial runtime/configuration changes.

Required contents:

```text
Tier 1
all retained optional feature tests
cargo package dry-runs
full installed-wheel suite
selected corpus replay
artifact execution smoke
```

### Tier 3 — Manual deep/specialist verification

Purpose: security-sensitive or platform-specific qualification.

Possible contents, invoked only when relevant:

```text
filesystem race qualification
fault injection
stateful replay
extended fuzz campaigns
TLS abuse
proxy interoperability/desync corpus
Windows adversarial qualification
longer concurrency/soak tests
binary-size/performance comparison
```

Tier 3 is not a universal release gate. The change owner selects the relevant suites based on modified boundaries.

## Track A — Simplify routine Rust CI

### Preserve the two-job structure

Retain:

```text
rust
python
```

This separation provides useful failure isolation and cache behavior. Do not combine jobs solely to reduce YAML line count.

### Rust job required steps

Preferred routine Rust job:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
```

Adjust flags only where Cargo target-selection syntax requires it.

### Remove routine `--all-targets`

Do not compile Criterion benchmarks and every example as part of every Clippy run unless a target is an active compile contract.

If specific examples are public API compile samples, test those examples explicitly through existing compile tests or a narrow command. Do not use `--all-targets` as a substitute for deciding which targets matter.

### Client feature testing

Move full `client-tls` testing out of routine CI unless the client remains a primary supported release surface after Plan 105.

Preferred policy:

- core client unit tests that compile under default workspace settings remain routine where applicable;
- `cargo test -p eggserve-core --features client-tls` runs in `verify.sh full` and before manual release;
- changes touching client modules or client feature declarations run it locally before handoff;
- no path-filter workflow complexity is added.

If removing it from routine CI would leave the only compile coverage for a shipped feature, retain one compile/test command. Choose correctness over shaving one command, but document the decision.

### TLS feature testing

Retain server TLS testing in routine CI because:

- the Python compatibility extension always exercises TLS-capable server code;
- TLS feature drift is otherwise easy to miss;
- the command is bounded and directly relevant to a shipped interface.

Do not add a TLS platform matrix.

### Required acceptance criteria for Track A

- routine Rust CI has no benchmark/example compilation by accident;
- all normal workspace tests remain blocking;
- server TLS remains covered;
- optional client verification has one explicit documented tier;
- no new workflow or job is added.

## Track B — Keep Python CI focused on the installed artifact

### Required Python job

Retain one Ubuntu Python job that:

- selects the supported interpreter;
- builds the standalone binary needed by packaging;
- stages it into the package;
- builds the wheel;
- installs into an isolated environment;
- imports the installed package without source-tree leakage;
- runs the installed-wheel test suite;
- exercises static GET/HEAD and one custom handler lifecycle;
- exercises HTTPS using repository-owned fixtures.

Continue using `PYTHONNOUSERSITE=1` and an unset/controlled `PYTHONPATH`.

### Do not split Python tests into multiple CI jobs

Do not reintroduce separate jobs for:

- packaging smoke;
- lifecycle parity;
- wire behavior;
- boundary hardening;
- TLS;
- static compatibility.

The installed-wheel script is the single authoritative Python CI entry point.

### Test count discipline

Do not treat total test count as a quality target. Consolidate duplicated facade tests when the same behavior is already proven at a lower native boundary and one installed-wheel integration test establishes wiring.

Retain direct installed-wheel tests for Python-specific concerns:

- constructor and lifecycle semantics;
- subclass dispatch;
- response-size bounds;
- exception sanitization;
- address tuple compatibility;
- static MIME override hooks;
- TLS constructor and scheme behavior;
- context management/shutdown.

### Acceptance criteria for Track B

- Python CI tests the wheel, not the checkout package;
- one job and one script remain authoritative;
- no platform matrix is added to PR CI;
- Python-specific behavior remains directly tested.

## Track C — Consolidate fuzz targets

### Objective

Reduce the 21-target fuzz surface to a smaller set aligned with actual security and protocol boundaries. Preserve deterministic property tests and regression corpora where they remain valuable.

### Retain high-value fuzz boundaries

Retain or merge into approximately these target groups:

#### 1. Request target and path confinement

Cover in one or a small bounded set:

- origin-form request target parsing;
- percent decoding;
- path component normalization;
- parent/current components;
- NUL and invalid UTF-8;
- separator ambiguity;
- dotfiles;
- Windows reserved/prefix/alternate-stream components.

It is acceptable to retain separate path targets when each reaches a materially different parser seam. Avoid three targets that merely feed the same final parser with different names.

#### 2. Range and conditional planning

Retain:

- range parsing and clamping;
- unsatisfiable ranges;
- `If-Range` validator behavior;
- `If-None-Match`/ETag matching;
- planner status/header/body invariants.

Merge conditional targets where a shared structured input can cover the same planner.

#### 3. HTTP field/framing conversion

Retain coverage for:

- method token validation and preservation;
- header name/value validation;
- duplicate header behavior;
- Content-Length reconciliation;
- response normalization;
- hop-by-hop/runtime-owned fields;
- no panic or semantic fallback.

Prefer one structured canonical request/response boundary target over separate trivial constructor targets.

#### 4. Request-body state machine

Retain while generic body primitives remain supported:

- one-shot consumption;
- configured limits;
- disconnect/incomplete states;
- cancellation;
- buffer/stream transitions exposed to the target seam.

#### 5. Directory listing encoding

Retain coverage for:

- HTML escaping;
- percent-encoded hrefs;
- response-size accounting;
- no partial body on overflow;
- control-character handling.

### Candidates to remove or merge

Review and normally merge/remove independent targets whose invariants are already subsumed:

- standalone status-code constructor fuzzing;
- separate method constructor plus method validation targets;
- separate header-block, header-name, and header-value targets when one structured target reaches all;
- event JSON serialization fuzzing;
- separate response builder and content-length targets when response normalization target covers both;
- URL/client parser fuzzing from the server-focused portfolio if it belongs only to the frozen optional client surface.

Client-only fuzzing may remain in a client-specific manual command if the feature remains supported. It should not drive static-server verification ceremony.

### Corpus handling

For removed targets:

- migrate unique regression inputs into the retained target where semantically applicable;
- delete duplicate seeds;
- retain human-readable regression tests for previously fixed defects;
- do not retain empty target directories for historical counts.

### Property tests

Normal `cargo test` property tests remain appropriate for:

- pure parser invariants;
- normalization idempotence;
- range bounds;
- method/header token validity;
- listing encoding bounds.

Avoid duplicating every fuzz assertion as another property test. Keep deterministic tests for known regressions and important boundaries.

### Documentation

Update target inventory and commands in:

- `fuzz/README.md`;
- `docs/fuzzing.md`;
- `architecture/testing-and-conformance.md`;
- `AGENTS.md` only if it contains active counts;
- repository development skill only if it contains active target lists.

Do not celebrate target count reduction as the objective; explain boundary consolidation.

### Acceptance criteria for Track C

- retained targets cover path, planner, framing/normalization, body state, and listing encoding;
- removed target seeds are migrated or intentionally deleted;
- no scheduled fuzz workflow is added;
- normal CI does not install cargo-fuzz;
- target count and commands are accurate.

## Track D — Reconcile `verify.sh`

### `fast`

Keep `fast` as the routine local equivalent of Rust CI:

```text
format
normal Clippy targets
workspace tests
```

Do not include:

- wheel builds;
- package dry-runs;
- client TLS;
- corpus replay;
- proxy tests;
- fuzzing;
- benchmarks.

### `full`

Keep `full` as the pre-release/substantial-change path:

```text
fast
server TLS
client TLS if retained
installed-wheel tests
cargo package dry-runs
```

Do not automatically install missing tools. Fail with a clear requirement when the selected mode needs them.

### `deep`

`deep` should invoke only deterministic expensive suites available in the local environment and should clearly skip optional external-tool checks.

Acceptable contents:

- selected corpus replay;
- stateful body replay;
- fault injection;
- filesystem race qualification;
- TLS abuse;
- proxy interoperability when Caddy/nginx are present.

Do not imply that `deep` is required for every change or every release.

### Specialist commands

Document standalone commands for:

- extended fuzz campaigns;
- Windows adversarial qualification;
- binary-size comparison;
- long soak tests.

Do not add them to the universal deep script if they require uncommon environments.

### Acceptance criteria for Track D

- each verification mode has one clear purpose;
- `fast` stays small;
- `full` covers shipped optional features and packaging;
- `deep` remains manual and environment-aware;
- scripts contain no evidence generation or implicit tool installation.

## Track E — Improve manual release artifact smoke validation

### Release policy

Publication remains manual. The GitHub release workflow, if retained, may build cross-platform wheel artifacts only through explicit `workflow_dispatch`.

Do not add:

- automatic tag publication;
- automatic PyPI publication;
- crates.io publication;
- release cadence triggers;
- scheduled release builds.

### Required platform builds

Retain the currently supported manual wheel targets:

- Linux x86_64;
- macOS arm64;
- Windows x86_64.

Do not add more targets in this corrective plan. Linux aarch64 and macOS x86_64 may remain documented source-supported/platform-supported distinctions according to existing policy.

### Add minimal installed-wheel smoke per platform

After building each wheel, install it into a clean environment on the same runner and run a bounded smoke suite:

```text
import eggserve
locate bundled binary
run --version
start loopback server on ephemeral port
GET one static file
stop server
```

Where supported in the existing fixture setup, also perform one native facade import/construction check. Do not run the complete installed-wheel suite on every release platform unless it remains reliable and inexpensive.

### Windows smoke

The Windows smoke must verify executable naming/path discovery and clean shutdown. It does not replace the separately documented Windows adversarial filesystem qualification limitation.

### Artifact upload

Upload built wheels for manual inspection/download. No evidence manifest is needed.

### Failure semantics

A smoke failure blocks that manual build run. Do not use `continue-on-error`.

### Acceptance criteria for Track E

- each built platform artifact is installed and executed before upload;
- release remains manually invoked;
- no publishing credentials or OIDC publication permission are introduced;
- no new target matrix is added beyond current builds.

## Track F — Reconcile active documentation

### Scope of audit

Review active documentation affected by Plans 103–105, at minimum:

- `README.md`;
- `AGENTS.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- `architecture/overview.md`;
- `architecture/configuration.md`;
- `architecture/runtime.md`;
- `architecture/eggserve-core.md`;
- `architecture/eggserve-bin.md`;
- `architecture/eggserve-python.md`;
- `architecture/testing-and-conformance.md`;
- `architecture/error-taxonomy.md`;
- `docs/api-stability.md`;
- `docs/cli.md`;
- `docs/dependency-policy.md`;
- `docs/fuzzing.md`;
- `docs/http-primitives.md`;
- `docs/non-goals.md`;
- `docs/python-api.md`;
- `docs/python-http-server-compatibility.md`;
- `docs/release-process.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `docs/tls.md`;
- `fuzz/README.md`;
- `benchmarks/binary-size.md` or selected measurement record;
- `.github/workflows/ci.yml`;
- `.github/workflows/release.yml`;
- Plans 102–106 status sections after implementation.

### Required final statements

Active docs must consistently state:

- EggServe is a hardened static server and bounded HTTP primitive/runtime library;
- custom services do not initialize a filesystem root;
- static service owns confinement/policy;
- runtime owns sockets, framing, deadlines, connection admission, and file-stream admission;
- service body policy is honored within runtime limits;
- static GET/HEAD bodies are rejected;
- incomplete/rejected content closes the connection;
- every listed `RuntimeConfig` field is effective;
- `--quiet` and `--log-format none` semantics are accurate;
- default index order is `index.html`, then `index.htm`;
- retained listing limits are accurately described;
- removed fields are absent from examples and API inventories;
- the client surface is frozen/low-level and not a primary Python API;
- routine CI has two jobs;
- release builds and publication are manual;
- deep verification is selected based on changed boundaries;
- binary-size values and build profile are reproducible;
- plans 000–106 are historical/currently complete only after final verification.

### Counts and paths

Update test, fuzz, corpus, plan, and file counts only where active docs rely on them. Prefer removing volatile exact counts when they provide little operational value.

Do not add another automated contract-consistency checker to prevent drift. Keep active docs smaller and easier to maintain instead.

### Historical documents

Historical plans and closure reports may retain historical language. Add a concise supersession note only where an old active claim would otherwise mislead implementation agents.

Do not rewrite 100 prior plans.

### Acceptance criteria for Track F

- active docs describe the actual final code;
- no stale removed field or target remains in commands/signatures;
- no automated publication claim remains;
- no new documentation subsystem is created.

## Track G — Final regression selection

### Required focused suites

Run focused tests for the corrected areas:

#### CLI/static/configuration

- logging modes;
- semaphore upper bounds;
- listing entry/response bounds;
- `index.htm` fallback;
- rejected-body close behavior;
- wheel license metadata.

#### Runtime/service ownership

- custom server without static root;
- static and custom shared transport path;
- file-stream permit ownership/saturation;
- service-declared body policy;
- TRACE/framing rejection;
- request conversion fail-closed;
- keep-alive/server-header behavior;
- removed fields absent from public compile samples.

#### Python facade

- installed-wheel six-class compatibility;
- custom/static lifecycle;
- response and callback bounds;
- address metadata;
- TLS custom/static smoke;
- no fake root for custom handler.

#### Size/build

- default and TLS dist artifact smoke;
- final feature graph;
- recorded before/after sizes;
- selected current-thread/multithread behavior tests.

### Security regression suites to retain

At minimum retain deterministic coverage for:

- traversal and percent-decoding attacks;
- symlink/root escape;
- Windows component denial;
- range and conditional correctness;
- TE/CL and duplicate length handling;
- response header/body normalization;
- file-stream permit lifetime;
- handler exception containment;
- listing escaping and bounds.

### Deep suites to select

Run filesystem race qualification if state/root refactoring touched confinement internals beyond ownership fields.

Run fault/stateful body suites because Plan 104 changes body-policy flow.

Run TLS abuse if TLS connection setup/finalization changed.

Run proxy interop only if framing/final response behavior changed in a way visible through a reverse proxy and tools are available.

Extended fuzz campaigns are optional manual confidence checks, not closure blockers unless a changed parser boundary warrants them.

## Track H — Same-commit closure procedure

### Step 1 — Clean repository state

Confirm:

```sh
git status --short
```

is clean before final validation.

### Step 2 — Local fast validation

Run:

```sh
./scripts/verify.sh fast
```

### Step 3 — Local full validation

Run:

```sh
PYTHON=python3.14 ./scripts/verify.sh full
```

Use the repository's supported invocation form if environment assignment differs.

### Step 4 — Selected deep validation

Run the relevant Plan 104 body/fault suites and any other changed-boundary suites selected in Track G. Record exact commands and results in the closure record.

Do not claim unrun optional suites passed.

### Step 5 — Artifact measurements

Rebuild final dist artifacts from the same commit and update the final binary-size table.

### Step 6 — Push final commit

Push implementation, tests, docs, and closure record.

### Step 7 — Hosted CI

Verify the exact final commit has passing:

```text
rust
python
```

Do not use results from a parent or later docs-only commit.

### Step 8 — Final closure record

Create one concise closure record, preferably:

```text
release/plan-102-106-closure.md
```

or update an existing current closure location if the repository has consolidated such records.

The record must include:

- final commit SHA;
- plans implemented;
- key defects fixed;
- removed and retained public configuration fields;
- final runtime/static ownership summary;
- accepted/rejected size optimizations with measurements;
- CI/fuzz work removed, retained, or moved to manual;
- local commands and results;
- hosted job results;
- remaining documented limitations, especially Windows adversarial qualification;
- explicit statement that no automated publication or scope expansion was introduced.

Do not create machine-readable evidence bundles.

### Step 9 — Mark plans complete

Update Plans 102–106 status only after hosted CI passes on the final commit.

## Unified final acceptance criteria

The Plan 102 roadmap is complete only when all of the following are satisfied.

### Correctness

- CLI logging flags are truthful.
- semaphore construction cannot panic from validated configuration.
- listing limits are honest and enforced.
- static index fallback is consistent.
- rejected bodies close without fixed draining.
- request conversion never silently changes semantics.
- static and custom body policy is layered correctly.

### Architecture

- custom services require no static root.
- one transport path serves static and custom services.
- one runtime file-stream semaphore governs canonical file responses.
- every retained runtime field is effective.
- no inert compatibility shim remains.

### Scope and size

- no supported feature was removed.
- no new application-server/client feature was added.
- final artifact measurements are reproducible.
- accepted size changes are measured and behavior-preserving.
- default CLI excludes optional TLS/client code.

### Verification

- routine CI has two jobs only.
- Python CI tests the installed wheel.
- fuzz targets are consolidated around meaningful boundaries.
- deep suites remain manual.
- manual release artifacts are installed and smoked before upload.
- publication remains manual.
- local fast/full and selected deep checks pass.
- hosted Rust/Python checks pass on the same final commit.

### Documentation

- active docs describe the final code.
- removed fields/targets/commands are absent.
- plan range and status are accurate.
- final closure record is concise and truthful.

## Explicit rejection criteria

Reject closure if:

- a required check is skipped but reported as passed;
- CI grows beyond two routine jobs;
- a scheduled or publication workflow is added;
- all-target benchmark/example linting remains without a documented need;
- fuzz targets are deleted without migrating unique regression inputs;
- Python tests run against source checkout rather than the installed wheel;
- release artifacts are uploaded without execution smoke;
- size claims compare incompatible artifacts;
- documentation claims removed configuration remains supported;
- Windows qualification is overstated;
- the final hosted checks belong to a different commit;
- a new plan/evidence system is introduced to close this plan.

## Handoff completion

After this plan is implemented and the same-commit closure criteria pass, Plans 102–106 may be marked complete. Subsequent work should return to normal issue/PR-scale maintenance rather than opening another numbered roadmap for minor polish.
