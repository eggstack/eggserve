# Plan 133 — Rust Library and CLI Usability Closure

## Status

**COMPLETE — 2026-08-15.**

Governing roadmap: Plan 128.

Depends on: Plan 129 external-consumer qualification, Plan 130 consolidation, and Plan 132 canonical examples.

Python remains EggServe's primary compatibility target, but EggServe is also intentionally structured as a Rust library plus a thin CLI. This plan closes the usability gap between "the Rust API exists" and "a downstream Rust project can reasonably discover, depend on, configure, run, and maintain it."

This is an ergonomics/packaging pass, not a framework-design pass.

---

## Target Rust user stories

A downstream Rust user should be able to accomplish all of these without importing EggServe internals or Hyper directly:

### Story 1 — hardened static server

```text
add eggserve-core dependency
construct safe/static configuration
bind loopback
serve a directory
obtain actual local address
shut down cleanly
```

### Story 2 — simple custom HTTP service

```text
construct Server/RuntimeConfig
supply Service/service_fn
inspect canonical Request
return canonical Response
use EggServe transport/framing/timeouts
shut down cleanly
```

### Story 3 — primitives without owning a server

```text
import eggserve_core::primitives
use canonical types / policy / response-planning primitives
avoid internal fs/path/response modules
```

### Story 4 — command-line server

```text
cargo install eggserve-bin (or documented repository/source equivalent)
eggserve --directory public
use secure defaults
opt into public bind/listing/symlinks only explicitly
```

The CLI must remain a thin product surface over the same core policies rather than a separate implementation.

---

## Track A — Audit public Rust package boundaries

Review `eggserve-core` exports and crate-level docs from the perspective of an external consumer.

Current intended buckets are already documented:

```text
stable-ish / semver-considered: config, limits, policy
intended public facade: primitives
experimental embedding surface: server
internal: fs, path, response, MIME internals
optional: tls
```

Verify that this division is reflected by actual visibility and examples.

### Required checks

- public examples do not require `pub(crate)` modules;
- common static embedding does not require reaching through undocumented module internals;
- common custom service handling does not require constructing Hyper types;
- errors needed to handle startup/shutdown/service failures are publicly reachable;
- `ServerHandle` exposes the information an embedder reasonably needs (readiness, address, shutdown/wait lifecycle) without exposing raw listener internals;
- relevant builders have usable defaults and controlled validation errors.

### Narrow re-export rule

If the external consumer from Plan 129/132 needs an internal type solely because a supported public operation returns or accepts it, add the narrowest sensible re-export at the owning public facade.

Do not make `fs`, `path`, or `response` wholesale public.

### Acceptance criteria

- [ ] static example imports supported public modules only;
- [ ] custom-service example imports supported public modules only;
- [ ] external consumer compiles without direct Hyper dependency;
- [ ] no broad visibility expansion occurs;
- [ ] stability comments match actual intended support.

---

## Track B — Static-server ergonomics audit

Use the canonical Rust static example as the usability test.

The desired flow should remain close to:

```rust
let runtime = RuntimeConfig::builder()
    .bind("127.0.0.1:8000".parse()?)
    .build()?;

let server = Server::builder()
    .runtime(runtime)
    .static_service("public")
    .build()?;

let handle = server.start().await?;
handle.ready().await?;
println!("{}", handle.local_addr());
...
handle.shutdown().await?;
handle.wait().await?;
```

Adapt this to the real API. Evaluate friction using these criteria:

```text
number of EggServe types needed
need for Arc wrapping by the caller
need to manually construct policy objects for defaults
need to import internal implementation types
need to duplicate CLI-specific conversion code
ability to use port 0
ability to retrieve bound address
shutdown clarity
error clarity
```

### Allowed polish

Make small public-API ergonomic corrections only if they remove concrete friction demonstrated by the example/external consumer, such as:

- a missing builder convenience for an already-supported configuration;
- a missing narrow public re-export;
- clearer constructor naming;
- a helper that eliminates repeated internal conversion already performed by CLI/Python.

### Disallowed polish

Do not add:

- a router;
- middleware layers;
- application state framework;
- async trait dependency solely for ergonomics;
- a new config serialization format;
- a new top-level facade crate;
- macros for server declaration;
- another runtime abstraction.

### Acceptance criteria

- [ ] static server example is concise enough for README use;
- [ ] safe defaults are available without constructing numerous policy knobs;
- [ ] port 0 and actual address publication work for embedders;
- [ ] shutdown/wait lifecycle is obvious and tested;
- [ ] no new framework abstraction is introduced.

---

## Track C — Custom `Service` ergonomics audit

The custom service boundary is valuable because it lets Rust projects reuse EggServe's HTTP correctness/runtime without forcing application-framework scope.

Use `crates/eggserve-core/examples/custom_service.rs` and the external consumer from Plan 129 as the test.

The example should prove:

```text
canonical Request is sufficient to inspect method/path/headers/body policy
canonical Response can represent normal small responses
ServiceError conversion is understandable
runtime owns framing and connection behavior
request body policy is explicit where relevant
```

If `service_fn` variants are confusing or overabundant, document their distinctions first. Consolidate only if there is clear duplication and no compatibility cost.

Do not add a high-level route table. A `match` in the example is sufficient.

### Acceptance criteria

- [ ] a downstream closure-based service can be written with public API only;
- [ ] simple response construction is documented;
- [ ] request-body behavior is explicit rather than surprising;
- [ ] runtime-owned headers/framing remain protected;
- [ ] no direct Hyper response/request construction is required;
- [ ] no router/framework feature is added.

---

## Track D — Rustdoc quality and compilation

Audit public rustdoc for the core surfaces used by consumers:

```text
crate root
config
policy
limits
primitives
server
RuntimeConfig/RuntimeConfigBuilder
Server/ServerBuilder/ServerHandle
Service/service_fn
StaticService/StaticServiceBuilder
canonical Request/Response types
TLS module/feature where public
```

### Required changes

- convert useful `ignore` examples to compiling `no_run` or normal doctests where feasible;
- ensure examples include required Tokio runtime setup when necessary;
- link public types using rustdoc intra-doc links;
- clearly mark `server` experimental before 1.0 without describing it as internal;
- preserve security caveats on file-handle/path extraction APIs;
- remove stale references to historical migration plans from rustdoc.

### Verification

```sh
cargo test --doc -p eggserve-core
cargo check -p eggserve-core --examples
```

Warnings must remain clean under the normal toolchain.

### Acceptance criteria

- [ ] crate docs explain which module a new consumer should start with;
- [ ] static/server doctests compile where practical;
- [ ] custom-service doctests compile where practical;
- [ ] experimental stability status is explicit;
- [ ] no misleading promise of 1.0 stability is introduced.

---

## Track E — Cargo package usability

Run the package path as a consumer would receive it.

### Required commands/evidence

Use the existing package verification script where applicable, plus direct Cargo package inspection:

```sh
bash scripts/verify-cargo-packages.sh
cargo package -p eggserve-core --allow-dirty   # exact flags may adapt to workspace/package constraints
cargo package -p eggserve-bin --allow-dirty
```

Inspect package contents and verify:

- README/license metadata is included correctly;
- examples/rustdoc references do not depend on omitted repository files;
- crate versions/path dependency declarations are publishable in shape;
- `eggserve-bin` depends on `eggserve-core` by version as well as local path appropriately;
- no test-only fixture is accidentally required at runtime;
- Cargo.lock policy is intentional for binary vs library packaging.

Do not publish as part of this plan.

### External packaged-consumer test

Where practical, unpack the generated `.crate` artifact or use a temporary local registry/package-path technique to compile the Plan 129 consumer against package contents rather than the source workspace. If the tooling cost is disproportionate, a package dry-run plus clean external path consumer is sufficient; document which was performed.

### Acceptance criteria

- [ ] core package dry-run passes;
- [ ] binary package dry-run passes;
- [ ] packaged examples/docs do not reference missing files;
- [ ] no publication automation is introduced;
- [ ] package metadata clearly identifies repository/license/description.

---

## Track F — CLI usability and parity with library policy

Audit the CLI as a user-facing simple server, not as a generic administrative framework.

### Required behaviors

Keep these obvious and documented:

```text
eggserve [PORT]
eggserve --directory DIR
eggserve --bind HOST
eggserve --port PORT
eggserve --public
security-policy opt-ins
resource/timeouts options
--help
```

Ensure the CLI defaults are derived from or remain behaviorally aligned with core `ServeConfig`/policy/limits rather than maintaining duplicated semantic defaults.

Manual argument parsing is intentional and should remain unless a concrete bug justifies changing it. Do not add clap for convenience.

### CLI/library parity audit

For each major CLI option, identify the owning library configuration field/policy. If CLI-only conversion logic duplicates a public conversion already used by Python, consolidate narrowly at the library/binary boundary.

Do not expose every Rust runtime knob as a CLI option merely because it exists.

### Exit behavior

Verify:

```text
--help exits successfully
invalid args fail predictably
bind failure returns nonzero
invalid root/config returns nonzero
SIGINT/SIGTERM shuts down cleanly where supported
logs/errors do not expose untrusted raw data
```

### Acceptance criteria

- [ ] CLI remains dependency-light/manual-parser based;
- [ ] secure defaults align with core policy;
- [ ] common invocation is short and documented;
- [ ] invalid configuration fails clearly;
- [ ] no duplicate server implementation exists in the binary crate;
- [ ] no new feature is added solely for CLI symmetry.

---

## Track G — Decide whether `eggserve-bin::run_cli` is public Rust API

The binary crate currently exposes a library entry point used by the Python package to avoid bundling a duplicate executable. This is an implementation-sharing success, but its status for third-party Rust consumers should be explicit.

Choose one documented stance:

### Preferred stance — integration API, not general embedding API

`eggserve-bin::run_cli` exists so packaged frontends can invoke the exact CLI parser/behavior in-process. Rust embedders should use `eggserve-core`, while ordinary command-line users execute `eggserve`.

Document this and avoid promising broad semver stability for `run_cli` unless it is already intentionally public.

### Alternative stance — supported CLI library entrypoint

Only choose this if there is a real downstream use case and the signature/error/stdio ownership is suitable for stable reuse.

Do not duplicate the CLI parser in another crate to avoid making this decision.

### Acceptance criteria

- [ ] `run_cli` status is documented;
- [ ] Python packaging continues to use the shared implementation;
- [ ] Rust library docs point embedders to `eggserve-core`;
- [ ] no second CLI implementation is introduced.

---

## Track H — No facade-crate proliferation

Evaluate naming friction honestly: the library is called `eggserve-core`, while the executable is `eggserve-bin` and the command is `eggserve`.

Do **not** introduce a new top-level `eggserve` Rust facade crate unless a concrete external-consumer/package problem cannot be solved by documentation/re-exports.

Aesthetic desire for `use eggserve::...` is not sufficient justification for another crate, dependency layer, version synchronization surface, and release artifact.

If no concrete blocker exists, record explicitly that `eggserve-core` is the intended Rust library crate for the 0.x line.

---

## Track I — External consumer regression fixture strategy

After the temporary external-consumer qualification proves the package boundary, decide whether to retain a tiny consumer fixture in the repository.

Preferred options:

1. rely on Cargo examples + package dry-run if they cover the same boundary;
2. retain a very small `tests/consumer/` crate only if it catches workspace-visibility/package mistakes examples cannot catch.

Do not add a nested workspace, package manager, or CI job solely for this fixture.

If retained, run it under `verify.sh full`, not routine CI, unless it is effectively free and provides unique value.

---

## Required verification

At closure:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test --doc -p eggserve-core
cargo check -p eggserve-core --examples
cargo build --profile dist --locked -p eggserve-bin
cargo build --profile dist --locked -p eggserve-bin --features tls
bash scripts/verify-cargo-packages.sh
```

Also run the clean external consumer static and custom-service smokes from Plan 129.

If Python-facing shared CLI code changes, rerun the installed-wheel suite as well.

---

## Rejection conditions

Reject an implementation that:

- makes internal filesystem/path modules public wholesale;
- adds Hyper to public examples;
- introduces a router/middleware framework;
- adds clap or another argument parser without a concrete need;
- creates a facade crate for naming aesthetics;
- duplicates CLI parsing or server runtime logic;
- weakens safe defaults for ergonomics;
- hides that the Rust server API is experimental before 1.0;
- automates crates.io publication;
- expands routine CI substantially.

---

## Final acceptance criteria

Plan 133 is complete when:

- [x] `eggserve-core` is demonstrably usable from a clean external crate;
- [x] static embedding uses public API only and is README/example quality;
- [x] custom `Service` embedding uses public API only and no direct Hyper dependency;
- [x] crate rustdoc clearly identifies public/stability boundaries;
- [x] useful doctests/examples compile;
- [x] Cargo package dry-runs pass for core and binary crates;
- [x] CLI remains thin, secure-by-default, and dependency-light;
- [x] CLI options map coherently to core policy/config ownership;
- [x] `eggserve-bin::run_cli` support status is explicit;
- [x] no new facade crate is added absent a demonstrated blocker;
- [x] Python packaging remains functional if shared CLI code is touched;
- [x] no application-framework scope is introduced.

## Closure evidence — 2026-08-15

The closure pass made no runtime architecture expansion and retained the
public boundary as `eggserve-core::primitives` plus the experimental
`eggserve-core::server` module. `eggserve-bin::run_cli` is documented as
integration plumbing for the Python wheel, and no `eggserve` facade crate or
permanent external-consumer fixture was added.

| Surface | Command/test | Result |
|---|---|---|
| Rust format/lint/tests | `cargo fmt --all -- --check`; workspace clippy/tests; TLS clippy/tests | Pass |
| Rust docs/examples | `cargo test --doc -p eggserve-core`; `cargo check -p eggserve-core --examples` | Pass |
| Dist artifacts | `cargo build --profile dist --locked -p eggserve-bin`; same with `--features tls` | Pass |
| Package boundary | `ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all` | Pass; core publish dry-run and packaged binary graph |
| Clean external consumer | Temporary crate using only `eggserve-core` and Tokio; static and custom-service TCP smokes on port 0; primitives import; clean shutdown | Pass; no Hyper or internal modules |
| Documentation boundary | README, AGENTS.md, skill, architecture/runtime, core/bin/overview, primitives, API stability, release contract | Updated; stale fields and unsupported lifecycle claims pruned |

---

## Roadmap closure handoff

After this plan and Plans 129–132 are complete, append final evidence to Plan 128 and stop the broad post-closure track. Further work should be driven by concrete bugs, security findings, compatibility reports, or release maintenance rather than another general polish roadmap.
