# Plan 105 — Product-Surface Freeze and Measured Binary-Size Reduction

## Status

**IMPLEMENTED — FINAL REVALIDATION REOPENED BY PLAN 108.** Third execution plan under Plan 102.

This plan begins after Plan 104 establishes the final runtime/static ownership model. It reduces artifact size and dependency surface without removing supported behavior or broadening the product.

The plan is measurement-driven. It does not assume that a dependency, feature flag, runtime scheduler, or crate split is worthwhile until stripped artifacts demonstrate a meaningful improvement.

## Goal

Produce a smaller and more auditable EggServe distribution while retaining:

- default static CLI behavior;
- optional server TLS;
- the six-class Python `http.server` facade;
- the Python native extension;
- the bundled Python CLI executable;
- the existing Rust client feature where currently supported;
- Linux, macOS, and Windows packaging targets;
- panic containment for service tasks;
- all hardened static-serving guarantees.

Required outcomes:

1. reproducible baseline measurements exist for every shipped artifact class;
2. a dedicated distribution profile produces stripped size-oriented builds;
3. Tokio features are narrowed to actual crate responsibilities;
4. the standalone CLI current-thread runtime is evaluated and retained only if behavior/performance remain acceptable;
5. TLS remains absent from the default CLI dependency graph and artifact;
6. optional client code remains isolated from default server artifacts;
7. no feature is deleted solely to improve a size number;
8. accepted and rejected optimization experiments are recorded concisely;
9. public product scope is frozen against further client/application-server expansion.

## Governing constraints

- Do not use `panic = "abort"`.
- Do not remove TLS support.
- Do not remove the bundled Python CLI.
- Do not remove the native Python extension.
- Do not remove existing client primitives in this plan.
- Do not reduce supported platforms.
- Do not replace Hyper, Tokio, rustls, PyO3, or rustix wholesale.
- Do not add a permanent binary-size CI gate.
- Do not add a benchmark dashboard or historical artifact database.
- Do not add UPX or runtime executable compression.
- Do not sacrifice response correctness, path confinement, panic containment, or graceful shutdown.
- Do not introduce unsafe code solely for size reduction.
- Do not split crates unless measurements demonstrate a real shipped-artifact benefit and the split reduces rather than increases maintenance complexity.

## Measurement model

### Artifacts to measure

Measure at least:

```text
A. default eggserve CLI, no TLS feature
B. eggserve CLI with tls feature
C. eggserve-core rlib metadata/build contribution where useful
D. Python _native extension
E. bundled CLI staged in the Python package
F. final platform wheel
```

On the primary development platform, record A–F where buildable. Manual release verification in Plan 106 records platform-specific wheel sizes.

### Build modes

Record both the current release profile and the proposed distribution profile:

```sh
cargo build --release --locked -p eggserve-bin
cargo build --release --locked -p eggserve-bin --features tls
cargo build --profile dist --locked -p eggserve-bin
cargo build --profile dist --locked -p eggserve-bin --features tls
```

For Python, use the same profile semantics through Maturin where supported.

### Normalization

Measurements must state:

- target triple;
- Rust toolchain version;
- feature set;
- profile;
- whether symbols were stripped;
- raw executable/extension size;
- final wheel size;
- Git commit.

Do not compare stripped and unstripped artifacts as if the difference came from code changes.

### Analysis tools

Use available local tools such as:

```text
ls/stat
file
size or llvm-size
cargo bloat
cargo tree -e features
```

These are developer tools, not repository dependencies. Do not add them to ordinary CI or require every contributor to install them.

### Acceptance threshold

Retain an optimization when it meets at least one of these conditions:

- reduces a primary shipped artifact by at least 2%;
- removes at least 100 KiB from a small artifact;
- removes a production dependency or major feature branch with neutral artifact size but materially improves auditability;
- simplifies duplicate runtime code while preserving performance and behavior.

Smaller changes may be retained when they are obvious simplifications with no downside. Do not contort code for sub-percent savings.

## Track A — Establish a clean baseline

### Required steps

1. Implement Plans 103 and 104 first.
2. Ensure the working tree is clean.
3. Build default and TLS CLI artifacts with the existing release profile.
4. Build the Python extension and wheel using the current packaging flow.
5. record dependency/features with `cargo tree -e features` for:
   - default CLI;
   - TLS CLI;
   - Python binding crate.
6. run `cargo bloat` or equivalent on default and TLS CLI where available.
7. identify the largest code/dependency contributors.

### Required questions

The baseline record must answer:

- Does default CLI include rustls/ring code unexpectedly?
- Does default CLI include client-only Hyper features?
- Does `eggserve-core` enable Tokio features needed only by the binary?
- How much size does the multithread Tokio runtime contribute relative to current-thread?
- How much wheel size comes from the extension versus bundled CLI?
- Are duplicate copies of Hyper/Tokio/rustls linked separately into the extension and bundled binary as expected?
- Does the PHF MIME map materially contribute?
- Are debug symbols already stripped in release wheels?

### Storage of results

Add one concise active document, preferably:

```text
benchmarks/binary-size.md
```

or update an existing benchmark artifact if one already serves this purpose.

Do not create per-run JSON evidence, dashboards, or generated historical directories.

The document should contain:

- baseline table;
- accepted changes;
- rejected/no-value experiments;
- final table;
- commands needed to reproduce.

### Acceptance criteria for Track A

- all size claims have comparable artifact data;
- default and TLS dependency graphs are known;
- no optimization begins from an unclean or incomparable build.

## Track B — Add a distribution profile

### Required profile

Add a size-oriented Cargo profile without changing routine developer release behavior unexpectedly:

```toml
[profile.dist]
inherits = "release"
opt-level = "s"
lto = "fat"
codegen-units = 1
strip = "symbols"
```

Evaluate `opt-level = "z"` against `"s"`; retain whichever produces the smaller artifact without a material throughput regression.

The exact LTO choice may be `thin` if it gives an equivalent size with substantially faster builds. Use measurements.

### Panic strategy

Retain unwinding. EggServe's generic runtime intentionally contains service panics at task boundaries. `panic = "abort"` would convert a recoverable service failure into process termination and is therefore prohibited.

### Packaging integration

Use the distribution profile for manual release artifacts only after:

- default CLI tests pass;
- TLS tests pass;
- Python wheel tests pass;
- artifact execution smoke tests pass.

Do not force contributors to use the dist profile for normal `cargo test` or iterative debug builds.

### Required measurements

Compare:

- existing release;
- dist with `opt-level = "s"`;
- dist with `opt-level = "z"` if tested;
- thin versus fat LTO if the difference is uncertain.

Record build-time observations qualitatively; do not build a performance telemetry system.

### Acceptance criteria for Track B

- one documented dist profile is selected;
- release artifacts are stripped;
- no panic or correctness semantics change;
- the selected profile has a measured benefit.

## Track C — Narrow Tokio feature ownership

### Current ownership principle

Crates should enable only the Tokio features they directly need.

Expected boundaries:

```text
eggserve-core
  runtime, net, time, fs/io, sync needed by reusable server/primitives
  no process signal handling

eggserve-bin
  macros/runtime plus signal handling and process lifecycle

eggserve-python
  runtime/net/io/sync/time required by native facade
```

### Required audit

For every enabled Tokio feature in all three crates:

- identify the production symbol/module that requires it;
- remove features used only by tests from production dependency declarations where practical;
- move test-only features to dev-dependencies if Cargo feature unification permits a real benefit;
- remove `signal` from core if only the binary handles signals;
- remove `rt-multi-thread` from crates that can operate with `rt` only;
- keep filesystem and I/O features required for file streaming.

### Cargo feature unification check

A manifest edit is not a size win if another dependency re-enables the same feature. Confirm the final feature graph with `cargo tree -e features`.

### Required tests

- default CLI builds from a clean target directory;
- TLS CLI builds;
- Python extension builds;
- workspace tests pass;
- no module accidentally depends on a transitive feature that is no longer guaranteed.

### Acceptance criteria for Track C

- crate manifests reflect actual responsibility;
- default CLI graph is no broader than required;
- no test-only feature remains in a production dependency without justification;
- changes are kept even if byte savings are small when they clearly improve dependency hygiene.

## Track D — Evaluate a current-thread standalone CLI runtime

### Rationale

The standalone static server is predominantly I/O-bound. A current-thread Tokio scheduler may reduce runtime code and binary size while retaining asynchronous socket and file behavior.

This is an experiment, not a predetermined change.

### Candidate implementation

Replace the standalone CLI's general runtime construction with a deliberate builder:

```rust
Builder::new_current_thread()
    .enable_all()
    .build()
```

The generic Rust library remains runtime-agnostic to the extent already intended. The Python facade may retain a multithread runtime because Python callback blocking and GIL scheduling have different requirements.

### Behavioral tests

The current-thread candidate must pass:

- normal static GET/HEAD;
- concurrent small-file requests;
- concurrent large-file streaming up to configured connection/file limits;
- range requests;
- TLS handshake and serving;
- slow client/header timeout;
- graceful shutdown while streams are active;
- logging and signal handling;
- connection admission behavior.

### Performance comparison

Use a bounded local benchmark representative of the product:

```text
small static file, moderate concurrency
large static file, moderate concurrency
range responses
TLS static file where available
```

Do not pursue maximum internet edge throughput. The decision threshold is suitability for local/LAN deployment.

Reject current-thread if it causes:

- obvious event-loop starvation;
- material throughput collapse under the default limits;
- poor latency when file operations or callbacks block;
- shutdown regressions;
- complex special cases.

A rough threshold of more than 10% degradation in representative throughput/latency should require a clearly larger size win and explicit justification. Prefer simplicity over a marginal byte reduction.

### Acceptance criteria for Track D

- current-thread is retained only with measured size benefit and acceptable local workload behavior;
- Python callback runtime remains correctly scheduled;
- no user-facing concurrency feature is removed.

## Track E — Confirm default feature isolation

### Server TLS

The default CLI must not include optional server TLS dependencies.

Verify:

- default dependency graph excludes rustls, tokio-rustls, rustls-pemfile, and ring where they are only TLS-related;
- default artifact contains no unexpected TLS symbols of consequence;
- `--tls-cert`/`--tls-key` remain feature-gated as documented;
- TLS artifact retains all current behavior.

### Client features

The default server artifact must not include:

- Hyper client legacy support;
- webpki roots;
- client TLS code;
- client URL/parser code unless shared by server primitives and proven necessary.

Because Cargo compiles at item reachability and feature level, confirm rather than assume.

### Python binding

The Python extension intentionally includes TLS for `HTTPSServer`. Do not remove it.

Document that the wheel contains:

- a native extension with TLS-capable compatibility server;
- a bundled standalone CLI whose TLS feature set depends on packaging policy.

If the bundled CLI is built without TLS while the extension provides HTTPS, document that difference accurately. Do not silently change it in this plan.

### Acceptance criteria for Track E

- optional features do not leak into default CLI;
- feature graphs match documentation;
- no supported feature is removed.

## Track F — Evaluate low-risk code-size contributors

Perform these only after Tracks B–E because compiler profile and Tokio runtime changes are likely higher leverage.

### MIME table

Measure PHF contribution.

Possible alternatives:

- generated `match` over extensions;
- sorted static slice plus binary search;
- retain PHF.

Retain the current implementation unless replacement:

- reduces artifact size measurably;
- keeps lookup code simple;
- preserves exact MIME behavior;
- does not add build scripts or generated-source machinery.

Do not reduce the supported MIME mapping to claim a win.

### Formatting and error strings

Do not manually abbreviate user-facing errors or remove diagnostics unless bloat data shows a meaningful contributor. Auditability and actionable errors matter more than small string savings.

### Generic monomorphization

Where bloat data identifies repeated generic transport bodies or helpers, consider small internal type erasure or shared functions. Do not add dynamic dispatch across the public service boundary solely for size.

### Duplicate static implementations

Plan 104 should already converge static and custom transport paths. Remove dead duplicate code after confirming no production caller remains. This is both a size and correctness improvement.

### Acceptance criteria for Track F

- every retained change has measured or clear structural value;
- MIME support and diagnostics remain intact;
- no build-generation subsystem is introduced.

## Track G — Freeze product surface

### Server scope freeze

After this roadmap, do not add server capabilities outside the existing non-goals without a new explicit product decision.

In particular, reject routine feature proposals for:

- routes/middleware;
- application handler ecosystems;
- uploads/forms/multipart;
- content compression;
- HTTP/2/3;
- WebSockets;
- reverse proxying;
- ACME and virtual hosts.

Correct HTTP/1.1 behavior, security fixes, platform hardening, and bounded compatibility corrections remain in scope.

### Client surface freeze

The existing client feature may remain for primitive completeness, but:

- do not expose a new primary Python client;
- do not pursue `httpx`/`requests` parity;
- do not add pools, redirects, cookies, proxies, authentication helpers, decompression, or HTTP/2;
- do not let client requirements enlarge the default server dependency graph;
- document it as low-level/experimental if that is its current status.

### Crate split decision

Do not move client code to a new crate automatically.

A split is authorized only if measurements and code ownership show all of the following:

- default server artifacts or compile graph materially benefit;
- public compatibility can be preserved or migrated simply before 1.0;
- workspace/release complexity does not increase disproportionately;
- the split removes real feature coupling rather than changing directory layout.

Otherwise retain feature-gated client code and freeze it.

### Python compatibility freeze

Retain the documented six-class subset. Do not pursue raw socketserver internals, `fileno()`, one-request listener mode, arbitrary stream replacement, forking mixins, or async handlers.

## Track H — Documentation and final measurements

### Required updates

Update:

- workspace/crate manifests with selected profile/features;
- `docs/dependency-policy.md`;
- `docs/non-goals.md`;
- `docs/release-process.md` for dist profile commands;
- `architecture/overview.md` or crate docs only where feature ownership changes;
- `benchmarks/binary-size.md` or the selected single size record;
- Python packaging docs where extension/bundled binary composition is clarified.

### Final table

Record before/after values for each measured artifact and calculate absolute and percentage change.

For each experiment, mark:

```text
accepted
rejected — no measurable win
rejected — performance regression
rejected — complexity cost
not applicable on this platform
```

Keep explanations brief and factual.

## Required verification

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features client-tls
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Also run:

- clean default and TLS dist builds;
- representative local static benchmarks for runtime-scheduler changes;
- artifact smoke execution after stripping;
- `cargo tree -e features` final checks.

Do not add the measurement tools or benchmark commands to ordinary CI.

## Completion criteria

Plan 105 is complete when:

- comparable baseline and final artifact sizes are recorded;
- a documented dist profile is selected;
- Tokio feature ownership is narrowed;
- current-thread CLI runtime is accepted or rejected with data;
- default CLI excludes optional TLS/client code;
- no feature has been removed;
- no unsafe or abort-based optimization is introduced;
- scope and client expansion are frozen in active documentation;
- all retained changes pass full local verification.

## Explicit rejection criteria

Reject the implementation if it:

- reports unstripped versus stripped changes as code-size wins;
- removes supported behavior;
- removes TLS or Python packaging targets;
- uses `panic = "abort"`;
- adds UPX or self-extracting artifacts;
- adds a permanent size CI gate;
- adds cargo-bloat as a production/dev dependency;
- splits crates without measured benefit;
- replaces core dependencies wholesale;
- keeps a current-thread runtime despite material local workload regression;
- reduces MIME coverage or diagnostics for negligible savings;
- expands the client feature while claiming scope cleanup.

## Handoff note

Proceed to Plan 106 only after the final retained artifact configuration is stable. Plan 106 owns CI/fuzz simplification, release smoke validation, documentation reconciliation, and same-commit closure.
