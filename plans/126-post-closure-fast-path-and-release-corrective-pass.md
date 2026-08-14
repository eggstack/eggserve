# Plan 126 — Post-Closure Fast-Path and Release Corrective Pass

## Status

**READY FOR HANDOFF — 2026-08-13.**

Reviewed baseline:

```text
main = 455fb5c076f2d940cc0ab5982bee6e29eba18f4c
```

This is a single narrow corrective pass after Plans 120–125. It is justified despite the earlier “no more polish plans” rule because the post-closure review found two concrete externally observable defects plus two evidence/documentation mismatches:

1. the stock `SimpleHTTPRequestHandler` native fast path can bypass the compatibility server’s intended concurrency bound;
2. `.github/workflows/release.yml` still stages the removed bundled CLI binary and references deleted `_find_binary()` behavior;
3. fast-path eligibility is looser than its documented `functools.partial` contract;
4. Plan 123/125 closure records contain claims stronger than the evidence retained in the repository.

This plan does **not** reopen product scope, Windows security architecture, CI strategy, or release cadence.

---

## Invariants to preserve

EggServe remains a hardened static HTTP/1.1 server with reusable Rust primitives, a bounded `http.server`-shaped Python facade, a Python-installed CLI backed by the native extension, a standalone Cargo CLI, and optional rustls TLS.

Do not add ASGI/WSGI, HTTP/2/3, reverse proxy behavior, application-server features, caches, new worker frameworks, dependency frameworks, new CI matrices, automated publication, or new Windows sandbox/security machinery.

Routine CI remains the existing small Rust + Python workflow. Release remains manually triggered.

---

## Track A — Restore compatibility concurrency semantics on the native fast path

### Problem

The compatibility facade configures:

```text
HTTPServer          -> max_workers = 1
ThreadingHTTPServer -> max_workers = configurable, default 8
```

For Python callback handlers, this is enforced through `max_python_callbacks`.

For the stock `SimpleHTTPRequestHandler` fast path, `callback=None`, so the callback semaphore is irrelevant. `_NativeServer` is then created without mapping the facade’s compatibility concurrency value into native admission, leaving native `max_connections` at its broader default.

The optimization can therefore make:

- `HTTPServer` no longer effectively serial for stock static serving;
- `ThreadingHTTPServer(max_workers=N)` ignore `N` for stock static serving;
- the same mismatch apply to the HTTPS variants.

This is an observable compatibility regression introduced by the optimization.

### Goal

Preserve the compatibility facade’s effective concurrency contract without adding another semaphore or scheduler.

### Preferred correction

Use the existing native connection admission limit for fast-path compatibility servers:

```text
HTTPServer                          -> native max_connections = 1
HTTPSServer                         -> native max_connections = 1
ThreadingHTTPServer(max_workers=N)  -> native max_connections = N
ThreadingHTTPSServer(max_workers=N) -> native max_connections = N
```

For callback-backed handlers, preserve the current callback semaphore behavior unless implementation review shows a small consistency correction is necessary.

Do not add a new core `max_request_concurrency` semaphore solely for this compatibility optimization.

If native admission cannot preserve the documented serial semantics closely enough, disable the fast path for the serial compatibility classes rather than weaken semantics.

### Required tests

Add production-boundary behavioral tests, not attribute-only tests:

```text
HTTPServer + stock SimpleHTTPRequestHandler
  -> two simultaneous held/slow static requests
  -> prove effective concurrency never exceeds 1

ThreadingHTTPServer(max_workers=2) + stock handler
  -> >=3 simultaneous held/slow static requests
  -> prove effective concurrency never exceeds 2

HTTPSServer + stock handler
  -> same serial proof over TLS

ThreadingHTTPSServer(max_workers=2)
  -> same bounded proof over TLS
```

Use deterministic synchronization where possible rather than fragile elapsed-time thresholds. Also verify subclass/custom callback concurrency remains unchanged.

### Acceptance criteria

- `HTTPServer` remains effectively serial with the stock fast path;
- `HTTPSServer` remains effectively serial with the stock fast path;
- `ThreadingHTTPServer(max_workers=N)` enforces `N` for stock native static serving;
- `ThreadingHTTPSServer(max_workers=N)` enforces `N` for stock native static serving;
- subclass/custom callback handling retains current bounded semantics;
- no new scheduler/semaphore abstraction is introduced;
- security, timeout, file-stream, and response-normalization behavior are unchanged;
- tests prove real concurrent request behavior.

---

## Track B — Make `functools.partial` fast-path eligibility exact

### Problem

The documented fast-path shape is intentionally narrow:

```text
SimpleHTTPRequestHandler
or
partial(SimpleHTTPRequestHandler, directory=...)
```

The implementation checks the resolved handler type and class attributes but does not reject unsupported `partial.args` or arbitrary extra `partial.keywords`. Constructor state that Python would normally see can therefore be silently bypassed by the Rust fast path.

### Required eligibility contract

Eligible handlers are only:

```text
SimpleHTTPRequestHandler
partial(SimpleHTTPRequestHandler)
partial(SimpleHTTPRequestHandler, directory=<path>)
```

For a partial:

- `.func` must be exactly `SimpleHTTPRequestHandler`;
- `.args` must be empty;
- keyword names must be a subset of `{ "directory" }`;
- captured `directory` must still pass existing root validation;
- request-relevant stock class attributes must remain at supported defaults.

Anything else must use Python fallback or fail through normal Python constructor behavior. Unsupported bound state must never be silently ignored.

### Required tests

```text
exact stock class                              -> eligible
partial(stock)                                 -> eligible
partial(stock, directory=tmp)                  -> eligible
partial(stock, unsupported_kw=value)           -> ineligible
partial(stock, directory=tmp, unsupported=...) -> ineligible
partial(stock, <bound positional arg>)         -> ineligible
subclass                                       -> ineligible
mutated unsupported class attributes           -> ineligible
```

Where an invalid partial would fail when invoked, prove fallback preserves that failure instead of silently serving through Rust.

### Acceptance criteria

- eligibility matches the documented shape exactly;
- unsupported partial arguments are never ignored;
- arbitrary subclasses remain Python-dispatched;
- no reflection-heavy or bytecode-based heuristic is added;
- eligibility remains a one-time server-configuration decision.

---

## Track C — Repair the manual release workflow to match Plan 122 packaging

### Problem

The actual Python packaging architecture is now:

```text
wheel
  -> Python package
  -> PyO3 _native extension
       -> shared eggserve-bin::run_cli()
  -> project.scripts entry point: eggserve
```

Routine installed-wheel CI uses this architecture and is green.

The manual `.github/workflows/release.yml` still implements the old architecture:

```text
cargo build -p eggserve-bin
copy eggserve[.exe] into python/eggserve/bin/
build wheel
import/use eggserve._bin._find_binary
```

`_find_binary()` no longer exists. The workflow should be considered stale/broken until corrected and dispatched successfully.

### Required workflow correction

Keep the existing targets only:

```text
Linux x86_64
macOS arm64
Windows x86_64
```

Remove steps whose only purpose is bundling a second executable:

```text
Build distribution binary   # when solely for wheel staging
Stage binary into package
_find_binary smoke logic
```

Build wheels directly from `eggserve-python`.

After installation into each smoke venv, explicitly verify both supported entry forms:

```text
<venv>/bin/eggserve --help              # Linux/macOS
<venv>/Scripts/eggserve.exe --help      # Windows
<venv-python> -m eggserve --help
```

Then serve a real fixture from the installed wheel. Reuse `scripts/release_smoke.py` in installed-command mode where practical rather than duplicating another server smoke harness.

The smoke environment must not resolve a source-tree or Cargo-target executable.

### Wheel composition assertion

Add a standard-library-only wheel member assertion proving the artifact does not contain:

```text
eggserve/bin/eggserve
eggserve/bin/eggserve.exe
```

Use `zipfile` or equivalent; do not add a dependency.

### Manual execution requirement

After correction, dispatch the existing `Release` workflow once from the implementation commit.

Required result:

```text
Linux x86_64  -> pass
macOS arm64   -> pass
Windows x86_64-> pass
```

If a platform exposes a small unrelated release defect, fix only that concrete defect when it remains inside the existing release contract. Do not redesign release infrastructure.

A successful hosted Windows wheel/smoke run proves packaging/runtime compatibility only; it is **not** Windows adversarial filesystem qualification.

### Acceptance criteria

- release workflow no longer stages a second executable into wheels;
- active tooling contains no `_find_binary()` reference;
- installed `eggserve` console script is exercised on all three release platforms;
- `python -m eggserve` is exercised on all three release platforms;
- real static fixture serving succeeds from each installed release wheel;
- wheel member inspection proves no bundled `eggserve[.exe]` server binary exists;
- manually dispatched Release workflow passes all three existing jobs;
- workflow remains `workflow_dispatch` only;
- no PyPI/crates.io publication step is added;
- no release matrix expansion is added.

---

## Track D — Reconcile Plan 123 performance evidence

### Problem

Plan 123 is marked complete and checks off both:

```text
baseline demonstrates material Python callback overhead
installed-wheel benchmarks demonstrate a repeatable benefit
```

but the repository does not retain the required before/after benchmark table.

### Required evidence reconstruction

Compare installed wheels from:

```text
pre-fast-path baseline: a3e12540ce0d9906899e344fb308611bdd8bf84d
corrected current implementation: Plan 126 implementation commit
```

Use the same host and lightweight benchmark harness for both.

At minimum measure:

```text
small GET
~64 KiB GET
HEAD
range
conditional 304
moderate concurrency
```

Run multiple samples and record medians plus enough context to distinguish signal from noise.

Use the original Plan 123 materiality rule: retain the fast path only if it demonstrates a repeatable meaningful benefit, approximately >=10% in throughput/CPU/tail behavior for at least a representative hot-path case.

If the corrected fast path does not show material benefit, remove it instead of retaining compatibility complexity simply because it already landed. Review any supporting redirect/status changes for independent HTTP correctness before removing them.

If benefit is demonstrated, append the compact evidence table to Plan 123. Do not add benchmark CI.

### Acceptance criteria

- actual before/after installed-wheel measurements are retained in Plan 123;
- benchmark conditions are comparable and described;
- fast path remains only if the original materiality threshold is met;
- no permanent benchmark gate/service is added.

---

## Track E — Minimal closure/documentation correction

Do not perform another broad documentation sweep. Correct only known stale claims caused by the implementation gap.

### Plan records

Append concise correction notes rather than rewriting history:

```text
Plan 122:
  packaging architecture was correctly deduplicated,
  but release.yml remained stale until Plan 126.

Plan 125:
  routine CI closure was valid,
  but it did not prove the then-stale manual release workflow.
```

After the manual Release workflow passes, record that evidence under Plan 126 and cross-reference it from those correction notes.

### Python docstrings/comments

Correct known stale bundled-binary wording:

- `serve_directory()` must no longer advertise `FileNotFoundError` for a missing bundled binary;
- `ServerProcess` must describe launching `sys.executable -m eggserve`, not wrapping a packaged binary;
- remove any remaining active `_find_binary`/packaged `bin/` comments.

### Windows status

Do not reopen Windows qualification. Keep the existing truthful posture:

```text
functional handle-relative confinement
independent adversarial qualification incomplete
do not use with untrusted mutable public content
```

### Acceptance criteria

- Plans 122 and 125 receive append-only correction notes;
- known stale Python bundled-binary wording is corrected;
- no broad docs cleanup is started;
- Windows security claims remain unchanged unless separate adversarial evidence exists.

---

## Verification

Run the normal verification posture after implementation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Focused required evidence:

```text
serial HTTPServer fast-path concurrency
bounded ThreadingHTTPServer fast-path concurrency
TLS equivalents
subclass callback concurrency regression
partial eligibility matrix
installed eggserve command
python -m eggserve
ServerProcess fixture
wheel member assertion: no bundled executable
Plan 123 before/after benchmark evidence
manual three-platform Release workflow dispatch
```

Routine CI shape must remain unchanged.

---

## Explicit acceptance criteria

Plan 126 is complete only when all are true.

### Fast-path correctness

- [ ] `HTTPServer` remains serial with stock native static serving;
- [ ] `HTTPSServer` remains serial with stock native static serving;
- [ ] `ThreadingHTTPServer(max_workers=N)` enforces `N` on the native fast path;
- [ ] `ThreadingHTTPSServer(max_workers=N)` enforces `N` on the native fast path;
- [ ] callback/subclass concurrency remains unchanged;
- [ ] no new concurrency primitive/framework is introduced;
- [ ] real concurrent-request tests prove the limits.

### Eligibility correctness

- [ ] exact stock handler is eligible;
- [ ] `partial(stock)` is eligible;
- [ ] `partial(stock, directory=...)` is eligible;
- [ ] bound positional args are ineligible;
- [ ] any keyword other than `directory` is ineligible;
- [ ] subclasses/mutated unsupported configuration remain ineligible;
- [ ] unsupported constructor state is never silently ignored.

### Packaging/release correctness

- [ ] release workflow no longer stages a standalone binary solely for wheel packaging;
- [ ] active tooling contains no `_find_binary()` reference;
- [ ] release wheels contain no `eggserve/bin/eggserve[.exe]` artifact;
- [ ] installed console script passes on Linux, macOS, and Windows release jobs;
- [ ] `python -m eggserve` passes on Linux, macOS, and Windows release jobs;
- [ ] real fixture serving succeeds from each installed release wheel;
- [ ] manually dispatched Release workflow passes all three existing jobs;
- [ ] release remains manual with no automated publication.

### Evidence/documentation truthfulness

- [ ] Plan 123 contains actual comparable before/after benchmark evidence, or the fast path is removed as unjustified;
- [ ] Plan 122 has an append-only correction note closing the stale release-workflow gap;
- [ ] Plan 125 has an append-only note clarifying the earlier manual-release evidence gap;
- [ ] stale bundled-binary Python docstrings/comments are corrected;
- [ ] Windows security claims are not strengthened by release smoke results.

### Full closure

- [ ] normal Rust/clippy/test/TLS verification passes;
- [ ] installed-wheel Python verification passes;
- [ ] routine CI remains the current small Rust + Python posture;
- [ ] no new dependency is added;
- [ ] no product-scope expansion occurs;
- [ ] no Plan 127 is created for residual cosmetic wording or speculative optimization.

---

## Rejection conditions

Reject an implementation that:

- keeps the fast path while knowingly changing `HTTPServer` serial semantics;
- adds a new core semaphore solely to repair this compatibility optimization when existing native admission can do the job;
- fast-paths arbitrary partials/subclasses through heuristic introspection;
- restores the bundled executable to simplify release tooling;
- reintroduces `_find_binary()` or PATH lookup;
- adds release jobs/platforms/publication automation;
- treats Windows wheel smoke as adversarial filesystem qualification;
- marks benchmark criteria complete without retaining measurements;
- rewrites historical plans to hide the prior gap;
- starts another dependency/security/CI cleanup track outside these findings.

---

## Recommended execution order

```text
1. Tighten partial eligibility.
2. Correct native-fast-path concurrency mapping and add behavioral tests.
3. Run focused + normal local verification.
4. Reconstruct Plan 123 before/after performance evidence.
5. Correct release.yml to the extension-backed wheel architecture.
6. Run installed-wheel/release smoke checks locally where possible.
7. Dispatch the existing manual Release workflow; require all three jobs to pass.
8. Append correction/evidence notes to Plans 122, 123, and 125.
9. Fix only the known stale bundled-binary Python docstrings/comments.
10. Run final normal verification and close Plan 126.
```

Do not create another roadmap. If implementation finds a new release-blocking defect directly caused by these corrections, record and fix it inside Plan 126 when reasonably scoped. Otherwise stop once these acceptance criteria pass.

## Final closure record (Plan 127) — 2026-08-14

Status: COMPLETE.

The corrective implementation and the platform-specific release-smoke fix are
complete on final commit
`b71ec982227a999d4bf530b2a4c0e8a8e4eaf538`. The authoritative closure evidence
is:

- Routine PR CI run [31836499452](https://github.com/eggstack/eggserve/actions/runs/31836499452) passed on this final implementation head; both `rust` and `python` jobs passed.
- Manual Release run [31836790126](https://github.com/eggstack/eggserve/actions/runs/31836790126) passed on this final implementation head:
  - Linux x86_64 job `94884761367`: success;
  - macOS arm64 job `94884761538`: success;
  - Windows x86_64 job `94884761552`: success.
- `test_callback_concurrency_is_bounded_at_public_server_boundary` passed in
  the installed-wheel Python suite. It exercised an ineligible custom handler,
  observed two active callbacks with `max_workers=2`, held the third callback
  out, and admitted it after a permit was released.
- The broken Linux `sys.modules["eggserve"]` smoke assertion was removed.
  Installed console-script/module help checks and real fixture serving remain.
- All three release jobs passed the wheel composition assertion proving that no
  standalone `eggserve/bin/eggserve[.exe]` is bundled.
- The Windows result proves wheel build, installation, entry-point, and fixture
  runtime compatibility only. Independent adversarial Windows filesystem
  qualification remains incomplete, and the existing warning is retained.

This append-only record closes the remaining Plan 126 criteria without
rewriting the historical plan body.
