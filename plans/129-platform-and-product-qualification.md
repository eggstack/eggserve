# Plan 129 — Platform and Product Qualification

## Status

**READY FOR HANDOFF — 2026-08-14.**

Governing roadmap: Plan 128.

Baseline:

```text
main = 4a2371045e221c6d3875f3a6085bd67fa53de7f5
```

This plan is qualification work, not a new hardening architecture phase. The goal is to execute EggServe at its actual product boundaries and make support claims match observed behavior. Correct only concrete defects found by qualification.

The highest-priority open question is Windows filesystem/security qualification. Existing Windows implementation and tests are substantial, but prior closure correctly stopped short of claiming adversarial qualification because the suite had not been executed on a real Windows host.

---

## Qualification surfaces

Qualification must cover five distinct product surfaces:

1. standalone Rust CLI;
2. installed Python console script / `python -m eggserve`;
3. Python `eggserve.server` static facade;
4. Python custom `BaseHTTPRequestHandler` facade;
5. external Rust consumer using `eggserve-core` public API.

These are not interchangeable. Passing a wheel smoke test does not qualify filesystem confinement, and compiling `eggserve-core` inside the workspace does not prove it is usable as an external crate.

---

## Track A — Linux product qualification

Use a clean Ubuntu/Linux environment representative of normal deployment.

### CLI qualification

Build or install a release-equivalent artifact and exercise:

```text
loopback default bind
explicit directory
port 0 if supported by the invoked surface
GET existing file
HEAD existing file
404 missing file
directory redirect
index.html/index.htm selection
single byte range
If-None-Match / If-Modified-Since behavior
request body rejection for static service
directory listing denied by default
dotfile denied by default
symlink denied by default
explicit opt-ins remain opt-in
clean SIGINT/SIGTERM shutdown
```

Do not use only in-process unit tests. At least one path must use a real TCP client against the built/installed server process.

### Python installed-wheel qualification

Build the wheel exactly through the current package path, install into a fresh venv, then prove:

```text
eggserve --help
python -m eggserve --help
real static file serving
from eggserve.server import HTTPServer, ThreadingHTTPServer
stock SimpleHTTPRequestHandler static serving
custom BaseHTTPRequestHandler response
clean server shutdown
```

The static facade test should confirm safe defaults and the native fast-path eligibility for the exact stock handler. The custom handler test should prove the Python callback path remains bounded and functional.

### Acceptance criteria

- [ ] qualification uses a clean environment rather than only the developer checkout;
- [ ] CLI serves a real fixture over TCP;
- [ ] static security defaults are exercised through the live server;
- [ ] range and conditional semantics are exercised through the live server;
- [ ] installed wheel console entry point works;
- [ ] `python -m eggserve` works from the installed wheel;
- [ ] stock Python static facade works;
- [ ] custom Python handler works;
- [ ] shutdown completes without leaked server processes;
- [ ] no product code change is made unless a concrete failure is observed.

---

## Track B — macOS qualification

Use a real macOS environment. Apple Silicon is the primary target because that is the documented prebuilt wheel target; x86_64 source support does not require a new routine CI runner for this plan.

Repeat the core product smoke matrix:

```text
CLI static serving
installed Python wheel
python -m eggserve
stock SimpleHTTPRequestHandler
custom BaseHTTPRequestHandler
safe default symlink/dotfile/listing behavior
range/HEAD
shutdown
```

Where macOS filesystem behavior differs from Linux, execute the existing confinement tests relevant to symlink traversal and file replacement races.

Do not add a permanent macOS job to routine CI. Existing manual release/qualification mechanisms are sufficient.

### Acceptance criteria

- [ ] Apple Silicon macOS release-equivalent CLI runs;
- [ ] macOS wheel installs and serves a fixture;
- [ ] Python facade behavior matches documented contract;
- [ ] default filesystem denial behavior is proven on macOS;
- [ ] no routine CI matrix expansion is introduced.

---

## Track C — Windows adversarial filesystem qualification

### Environment policy

A GitHub-hosted `windows-latest` runner counts as a real Windows execution environment for this qualification because the requirement is execution against Windows filesystem and handle semantics, not possession of a physical Windows workstation.

If no local Windows host is available, use a **manual-only** `workflow_dispatch` qualification path. Prefer one of these approaches, in order:

1. reuse an existing manually dispatched workflow if it can invoke the adversarial suite without conflating release smoke with security qualification;
2. add a small manual qualification job/workflow whose sole purpose is to run the existing Windows-specific/adversarial tests;
3. if a temporary workflow is used only to collect evidence, remove it after qualification unless repeatability clearly justifies retaining the manual workflow.

Do not add Windows adversarial qualification to every push/PR.

### Required test classes

Run the existing Windows confinement and adversarial tests that cover, where implemented by the repository:

```text
root handle pinning
child handle-relative resolution
symlink/reparse-point denial under safe defaults
junction/reparse escape attempts
replacement/race scenarios
case-insensitive path behavior
reserved/special path handling
path separator/backslash handling
dotfile policy behavior
handle-relative directory enumeration
file replacement after resolution
root replacement/removal behavior
long/odd path components already represented by the test suite
```

Do not invent a new Windows filesystem abstraction during this plan. If an existing test cannot run because GitHub-hosted Windows lacks a privilege required for symlink creation, record that limitation separately and use available non-privileged reparse/junction cases or an elevated/local host later. Do not mark the inaccessible test class as passed.

### Evidence standard

Record:

```text
runner/Windows version
filesystem used for temp fixtures
commit SHA
exact test command(s)
number of tests executed
passes/failures/skips
privilege-dependent skips
workflow run URL/ID if GitHub Actions is used
```

A normal Windows wheel build/smoke remains separate evidence and must not substitute for these tests.

### Support-language outcome

After execution, choose one of three outcomes:

**Outcome 1 — qualified:** all security-relevant tests run and pass with no material untested privilege-dependent class. Documentation may remove the current broad warning and replace it with a precise Windows support statement.

**Outcome 2 — functionally qualified, adversarial caveat remains:** ordinary Windows runtime and most confinement tests pass, but an important test class cannot execute. Keep a narrowed warning naming the remaining gap.

**Outcome 3 — defect found:** keep the warning and open/implement only the narrow corrective work required by the observed defect before stronger claims.

### Acceptance criteria

- [ ] Windows-specific/adversarial tests execute on a Windows host;
- [ ] exact test evidence is recorded;
- [ ] ordinary wheel smoke is not mislabeled as adversarial qualification;
- [ ] skipped privilege-dependent cases are explicitly identified;
- [ ] support language is changed only if evidence warrants it;
- [ ] routine CI remains unchanged or no broader than before;
- [ ] no speculative Windows rewrite is performed.

---

## Track D — External Rust consumer qualification

Workspace tests are insufficient proof that `eggserve-core` is usable as a library.

Create a temporary clean consumer crate outside the workspace and depend on EggServe by local path first. If package-dry-run tooling permits consuming the packaged crate artifact cleanly, repeat against the packaged form.

### Required consumer cases

#### Case 1 — static server

The consumer must use public APIs only and perform the conceptual flow:

```text
construct RuntimeConfig
construct Server builder
configure static service/root
start on loopback/ephemeral port
wait ready
issue real GET/HEAD from a client
shutdown
wait cleanly
```

No import from `pub(crate)` internals and no direct Hyper dependency is permitted in the consumer.

#### Case 2 — custom service

Use the public `Service`/`service_fn` boundary and canonical request/response types to return a small dynamic response. This proves the library can support simple HTTP logic without becoming an application framework.

#### Case 3 — primitives-only

Use `eggserve_core::primitives` for one concrete hardened operation that does not start a server, such as request/response canonical construction or safe static-resolution/planning API where publicly intended. This proves the primitives facade is independently understandable.

### Package qualification

Run the existing Cargo package dry-run checks and inspect the packaged file list. Confirm that public docs/examples referenced by crates.io packaging are included as intended and that no repository-only assumption is required to build the crate.

### Acceptance criteria

- [ ] a clean external crate compiles against `eggserve-core`;
- [ ] external static server runs over TCP;
- [ ] external custom service runs over TCP;
- [ ] consumer does not depend directly on Hyper;
- [ ] consumer does not import internal modules;
- [ ] package dry-run passes;
- [ ] any missing public re-export discovered is fixed narrowly, not by making internal modules public wholesale.

---

## Track E — Qualification evidence document strategy

Do not create a new evidence database or generated registry.

Record evidence in the closure section of this plan and, where support policy changes, in the existing normative support/security documentation. A short manually maintained table is enough.

Recommended closure table:

| Surface | Platform | Artifact | Command/test | Result | Evidence |
|---|---|---|---|---|---|
| CLI | Linux | dist binary | live static smoke | pass/fail | local record |
| Python | Linux | installed wheel | facade smoke | pass/fail | CI/local |
| CLI/Python | macOS arm64 | dist/wheel | smoke matrix | pass/fail | manual run |
| Windows filesystem | Windows x86_64 | source/tests | adversarial suite | pass/fail/partial | workflow URL |
| Rust library | Linux | external crate | static/custom service | pass/fail | command log |

---

## Corrective-change rule

If qualification finds a defect:

1. reproduce it with the narrowest possible test;
2. classify it as correctness, security, packaging, compatibility, or documentation;
3. fix the defect at the owning layer;
4. rerun only the affected qualification plus routine regression checks;
5. do not use the failure as justification for unrelated refactoring.

If a severe security defect appears, stop stronger support claims until corrected.

---

## Explicit non-goals

This plan must not:

- add permanent broad platform matrices to routine CI;
- redesign filesystem confinement absent a reproduced defect;
- add benchmark requirements;
- add HTTP/2/3;
- add ASGI/WSGI;
- expose raw Python sockets;
- expand the Rust service layer into routing/middleware/application features;
- claim Windows hardening from a wheel smoke test;
- require every deep test on every commit.

---

## Final acceptance criteria

Plan 129 is complete when:

- [ ] Linux CLI/Python live qualification passes;
- [ ] macOS arm64 CLI/Python qualification passes or a concrete environment limitation is truthfully recorded;
- [ ] Windows adversarial suite has been executed on Windows and outcome 1/2/3 is explicitly recorded;
- [ ] Rust external static consumer passes;
- [ ] Rust external custom-service consumer passes;
- [ ] package dry-run passes;
- [ ] support/security docs are not stronger than the evidence;
- [ ] any observed defects have focused regression tests;
- [ ] no unrelated scope expansion occurred.