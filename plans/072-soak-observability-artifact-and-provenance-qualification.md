# Phase 72 — Soak, Observability, Artifact, and Provenance Qualification

## Goal

Demonstrate stable long-duration operation under mixed hostile and ordinary traffic, finalize production observability semantics, and prove that published binaries and wheels—not only source-tree builds—correspond to the release source and pass the production-path test matrix.

## Preconditions

- Plan 071 completes cross-platform race and fault-injection qualification.
- Deterministic HTTP, proxy, TLS, body, and filesystem corpora are available.
- Release criteria and evidence aggregation already fail closed for required gates.

## Non-goals

Do not add:

- a network administration or metrics server;
- distributed tracing infrastructure;
- autoscaling/orchestration;
- a hosted monitoring product;
- performance claims based solely on peak RPS;
- application-level telemetry;
- automatic artifact publication without existing release approval controls.

## Track A — Production observability contract

Define stable structured event categories for:

- startup and effective profile;
- platform/filesystem classification;
- bound address and TLS state;
- connection admission and saturation;
- handshake success/failure/timeout;
- request completion;
- response status and bytes sent;
- request duration;
- framing/header/target/body rejection category;
- path/policy rejection category;
- file/listing admission saturation;
- callback saturation/timeout;
- keep-alive timeout/request maximum;
- graceful shutdown start/completion/deadline;
- forced cancellation;
- internal invariant/fault category.

Fields must be bounded and sanitized. Do not log:

- absolute local filesystem paths;
- raw control characters;
- private key material;
- authorization-like values;
- unsanitized request targets;
- raw forwarding headers as trusted identity;
- response body data.

Provide text and JSON behavior with parity tests. `none` logging must remain quiet except unavoidable process-fatal diagnostics as documented.

## Track B — Internal metrics/counter surface

Where useful, expose protocol-neutral counters through:

- internal test observer;
- optional library callback/snapshot;
- or structured logs.

Candidate counters:

- active/total connections;
- active handshakes;
- active file streams;
- active callbacks;
- rejection counts by category;
- timeout counts by category;
- bytes sent;
- graceful/forced shutdown counts.

Do not add an HTTP endpoint. Stable public exposure should be minimal and only if downstream embedding genuinely requires it. Test-only instrumentation may remain internal.

## Track C — Soak workload design

Build a reproducible mixed workload containing:

- small, medium, and large files;
- empty files;
- ranges and unsatisfiable ranges;
- conditional requests;
- 404/403/405;
- directory requests/listing where profile permits;
- connection churn;
- keep-alive reuse;
- slow headers;
- slow readers;
- malformed framing corpus;
- TLS handshake churn for direct-TLS profile;
- proxy traffic for origin profile;
- filesystem mutation subset;
- Python static server and callback subset where claimed;
- graceful and forced restart cycles.

Use deterministic proportions and record seed/configuration.

## Track D — Resource measurements

Collect at fixed intervals:

- resident/working-set memory;
- private/virtual memory where meaningful;
- allocator metrics if available without invasive changes;
- file descriptor or handle count;
- thread count;
- Tokio task count through test instrumentation;
- active permits by resource class;
- socket state counts;
- CPU utilization;
- request throughput;
- median and tail latency;
- timeout/rejection/error rates;
- shutdown duration.

Measure baseline warmup, steady state, and post-load recovery.

Acceptance should use trend/envelope rules, not brittle exact values. Any monotonic unbounded growth is a failure even if the process does not crash.

## Track E — Soak schedule

Define:

- PR smoke soak: short and deterministic;
- scheduled nightly soak: moderate duration;
- pre-release soak: at least 24 hours for each production profile, longer where infrastructure permits;
- Windows dedicated-runner soak for local NTFS;
- direct-TLS soak;
- reverse-proxy soak with pinned Caddy/nginx versions;
- Python installed-wheel soak subset.

Record source SHA, artifact hashes, OS, filesystem, proxy versions, TLS feature set, workload seed, duration, and result.

## Track F — Leak and degradation analysis

Automate checks for:

- descriptors/handles not returning near baseline;
- semaphore permits lost;
- active tasks after shutdown;
- memory slope beyond allowed stabilization envelope;
- increasing latency at constant load;
- connection states accumulating;
- callback workers accumulating;
- repeated restart degradation;
- log volume amplification under malformed traffic;
- stale temporary files or sockets.

Failures must produce time-series artifacts and the last relevant structured events.

## Track G — Installed artifact matrix

Build and install artifacts in clean environments:

- Linux x86_64 standalone binary;
- Linux aarch64 artifact where claimed;
- macOS arm64 and x86_64 artifacts where claimed;
- Windows x86_64 binary;
- CPython 3.14 wheels for each claimed platform/architecture;
- TLS-capable binary artifact where direct TLS is claimed.

Run outside the source tree:

- version/help;
- CLI smoke;
- static production-path suite;
- path/confinement subset;
- raw-wire subset;
- range/conditional subset;
- shutdown;
- platform-specific hardened subset;
- Python import/API snapshot;
- in-process server subset;
- subprocess `python -m eggserve`;
- TLS subset for TLS artifacts.

No source-tree import or binary fallback may satisfy installed-artifact gates.

## Track H — Artifact identity and reproducibility

For each artifact record:

- source commit SHA;
- Cargo.lock hash;
- Python lock/build metadata as applicable;
- Rust toolchain and target;
- Python interpreter ABI;
- feature set;
- build command;
- artifact SHA-256;
- contained binary/native-extension hashes;
- package metadata;
- build environment identifier.

Verify the Python wheel’s native extension and bundled CLI derive from the same source revision and intended feature set.

Add checks preventing stale binaries from being copied into a new wheel.

## Track I — SBOM and provenance

Generate:

- SBOM for Rust dependencies and packaged components;
- checksums manifest;
- artifact provenance/attestation using the repository’s release platform;
- dependency audit and license-policy evidence;
- release gate/evidence manifest tied to SHA.

Keep signing/attestation within existing platform capabilities. Do not build a custom PKI.

The evidence aggregator must reject:

- missing artifact hashes;
- source SHA mismatch;
- expired/stale evidence;
- artifact built before a security-relevant change;
- unrecognized feature set;
- skipped required platform tests;
- support profile without all required artifacts.

## Track J — Upgrade, install, and uninstall smoke

Test:

- clean installation;
- upgrade from prior supported prerelease/release where available;
- command discovery;
- `python -m eggserve`;
- package uninstall removing installed files appropriately;
- reinstall;
- no source-tree contamination;
- Windows executable locking behavior during upgrade/uninstall;
- macOS/Linux executable permissions.

Do not promise in-place zero-downtime upgrades; process managers/proxies own deployment orchestration.

## Track K — Performance characterization

Record conservative operational characteristics:

- memory baseline;
- throughput/latency under representative static workloads;
- large-file streaming behavior;
- connection saturation behavior;
- Windows versus Unix notes;
- reverse-proxy versus direct-TLS overhead.

These are characterization data, not universal benchmark claims. Security bounds and predictable degradation take priority over peak throughput.

## Required tests

- structured logging sanitization and schema;
- profile/startup event correctness;
- resource counter baseline/recovery;
- PR/nightly/pre-release soak harness tests;
- installed artifact matrix;
- source/artifact identity validation;
- wheel bundled-binary freshness;
- SBOM/checksum/provenance generation;
- evidence aggregator negative fixtures;
- install/upgrade/uninstall smoke;
- final shutdown cleanup.

## Release criteria

Add required gates for:

- profile-specific pre-release soak;
- resource trend analysis;
- installed binaries and wheels;
- artifact/source identity;
- SBOM;
- checksums;
- provenance/attestation;
- dependency audit/deny;
- logging sanitization;
- evidence aggregation.

Security-relevant gates must be non-waivable. Operational soak may allow a documented rerun for infrastructure failure, but not a waiver for missing evidence.

## Acceptance criteria

- Pre-release soaks complete for every claimed profile.
- Memory, descriptors/handles, tasks, sockets, and permits stabilize and recover.
- Malformed traffic does not create unbounded log or CPU amplification.
- Installed artifacts pass the production-path matrix outside source trees.
- Every artifact is tied to exact source, toolchain, feature set, and checksum.
- SBOM and provenance evidence are produced and aggregated.
- The release aggregator fails closed on stale, skipped, or mismatched evidence.

## Stop conditions

Do not proceed to final audit/release if:

- any resource trend remains unexplained or unbounded;
- installed artifacts differ materially from tested source builds;
- wheel and bundled binary source identity cannot be proven;
- required platform soak evidence is absent;
- logging leaks paths/control characters/secrets;
- provenance or checksums are missing for published artifacts.

## Handoff

Plan 073 consumes the complete evidence bundle, commissions or performs an independent review, closes findings, and authorizes only the production profiles whose exact gates pass on the final release SHA.
