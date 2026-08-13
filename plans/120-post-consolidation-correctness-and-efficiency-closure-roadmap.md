# Plan 120 — Post-Consolidation Correctness and Efficiency Closure Roadmap

## Status

**READY FOR HANDOFF — 2026-08-11.**

Baseline reviewed for this roadmap:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
```

This roadmap is a bounded follow-up to the completed Plan 112–119 consolidation track. It exists because a post-closure review found one concrete Python lifecycle correctness defect and several narrow opportunities to simplify distribution/runtime cost without expanding EggServe's product scope.

This is **not** another feature roadmap. It authorizes Plans 121–125 only. When those plans are closed, do not create another follow-up merely for stylistic cleanup, speculative optimization, or test-count growth.

---

## Product contract to preserve

EggServe remains:

```text
hardened static HTTP/1.1 server
    + safe-by-default filesystem confinement
    + correct static HTTP semantics
    + reusable Rust HTTP/security primitives
    + bounded Python http.server-shaped facade
    + Python-packaged CLI/subprocess convenience
    + optional standalone native TLS / HTTPS-capable Python facade
```

EggServe remains explicitly out of scope for:

```text
ASGI / WSGI
application-server framework behavior
reverse proxying
HTTP/2 / HTTP/3
WebSockets
ACME / virtual hosting
HTTP client product functionality
plugin systems
WAF/authentication/rate-reputation systems
container/sandbox orchestration
system-service management
```

No plan in this track may broaden those boundaries.

---

## Why this track is still justified after Plan 119

The reviewed tree is materially healthier than earlier versions, but five bounded issues remain.

### 1. Python readiness semantics are incorrect in one state

`crates/eggserve-python/src/server.rs::PyServer::wait_ready()` currently executes:

```rust
let _ = tokio::time::timeout(STARTUP_TIMEOUT, handle.ready()).await;
```

and then returns `Ok(())` for any post-wait state other than `Failed`, including a server that remains `Starting`. A timeout can therefore be reported as successful readiness.

`start()` already has its own bounded wait-to-Running loops, so the corrective work should remove duplicated readiness logic where practical rather than adding another lifecycle framework.

### 2. The Python wheel contains two Rust server artifacts

The wheel links the PyO3 native extension and also stages the standalone `eggserve-bin` executable into `python/eggserve/bin/`. `eggserve._bin` then launches that packaged executable. This duplicates the Rust HTTP/TLS implementation in a single Python distribution.

Plan 119 required the bundled binary to remain because that was the existing distribution contract at the time. Plan 122 **supersedes only that implementation detail**: CLI and subprocess functionality remain required, but the wheel no longer needs to carry a second Rust server executable if equivalent behavior can be provided through the already-linked native extension.

Historical Plan 119 text must not be rewritten.

### 3. The common Python static compatibility path crosses the GIL unnecessarily

`HTTPServer.server_activate()` currently installs `_handle_request` as a Python callback even when the handler is the stock `SimpleHTTPRequestHandler`. Every request therefore reaches Rust, enters Python to construct/dispatch the handler, then calls native `StaticResponder` and returns to Rust for transport.

The common source-compatible form:

```python
Handler = partial(SimpleHTTPRequestHandler, directory="public")
ThreadingHTTPServer(("127.0.0.1", 8000), Handler)
```

should be eligible for a native static path when doing so is behaviorally exact. Arbitrary subclasses must continue through Python.

### 4. Each Python `Server` constructs an unconstrained Tokio multi-thread runtime

`tokio::runtime::Runtime::new()` creates the normal multi-thread runtime for each native Python server instance. On high-core-count hosts or processes that create several servers this can produce avoidable thread/memory cost. Optimization must be measurement-driven and must not replace a simple per-server ownership model with difficult global runtime state unless there is overwhelming evidence.

### 5. Windows support and public documentation need evidence-based closure

Windows has handle-relative confinement and a substantial adversarial test scaffold, but `SECURITY.md` and the README correctly retain a warning that independent adversarial qualification is incomplete. The remaining task is to execute/record the strongest feasible Windows evidence, not invent more filesystem machinery in advance.

Public docs also still contain planning-history prose (for example Plan 108/109 narrative in the README) and need a precise distinction between source-supported platforms and platforms for which prebuilt wheels are actually produced.

---

## Non-negotiable invariants

### Filesystem/security

- safe-default Unix traversal remains descriptor-relative;
- safe-default Windows traversal remains handle-relative at its qualified level;
- configured-root confinement remains library-enforced;
- symlinks/reparse traversal remain denied by default;
- dotfiles remain denied by default;
- directory listing remains disabled by default;
- file-backed responses retain opened-handle/capability semantics through transport;
- no optimization may reintroduce pathname reopen races;
- Windows support must not be promoted beyond available evidence.

### HTTP correctness

- HTTP/1.1 remains the transport;
- GET/HEAD, conditional, range, framing, `Date`, body-forbidden status, and response normalization behavior remain unchanged;
- static service continues to reject request bodies by default;
- request-body limits and timeout semantics remain enforced for custom handlers.

### Python compatibility

- the six supported compatibility classes remain present;
- arbitrary `BaseHTTPRequestHandler` / `SimpleHTTPRequestHandler` subclasses retain Python dispatch semantics within the documented compatibility boundary;
- `python -m eggserve`, an installed `eggserve` command, `serve_directory()`, and `ServerProcess` remain usable;
- HTTPS classes remain available in the standard Python distribution;
- CPython 3.11 abi3 floor remains intact.

### Verification/process

- routine CI remains the small Rust + installed-wheel posture established by Plans 115/119;
- no permanent benchmark service/framework is added;
- no fuzz/race/deep suite becomes a routine merge gate merely because this track touches it;
- no automated release publication is added;
- no new dependency is allowed solely to make an optimization easier to implement.

---

## Plan sequence

### Plan 121 — Python lifecycle readiness correctness

Correct `wait_ready()` so it never reports success unless the lifecycle is `Running`. Prefer one internal readiness implementation shared by `start()` and `wait_ready()` to the current duplicated polling/timeout logic. Add deterministic regression coverage without 30-second sleeps.

This is the only unconditional implementation fix in the track and should land first.

### Plan 122 — Python wheel/CLI deduplication and artifact-size closure

Measure the installed wheel composition, then remove the bundled standalone Rust server executable if CLI/subprocess behavior can be preserved through the native extension. Prefer sharing the existing Rust CLI parser/executor between `eggserve-bin` and the PyO3 extension rather than reimplementing the CLI in Python. Preserve a standalone Cargo-installed `eggserve-bin` for non-Python use.

This plan may update Plan 119-era packaging assumptions, but must not rewrite Plan 119 history.

### Plan 123 — Native fast path for stock `SimpleHTTPRequestHandler`

Benchmark the current compatibility path. If the Rust→Python→Rust transition is materially costly, route the exact stock `SimpleHTTPRequestHandler` configuration directly through the Rust static service. Subclasses, overridden methods, or mutable configuration that cannot be proven equivalent must fall back to Python dispatch.

Do not optimize by weakening compatibility.

### Plan 124 — Python runtime worker right-sizing ✅

Measure thread/memory/runtime behavior of `Runtime::new()` for one and multiple Python servers. If the default per-server worker pool is materially excessive, use an explicitly bounded Tokio runtime configuration. Retain per-server ownership by default; do not introduce process-global runtime lifecycle complexity without strong evidence.

**Result:** Replaced `Runtime::new()` (host-core worker count) with `Builder::new_multi_thread().worker_threads(2)`. Reduced per-server threads by 78% on a 16-core host with no measurable throughput regression. Per-server ownership preserved.

### Plan 125 — Windows qualification, support truthfulness, and final documentation closure

Execute the existing Windows adversarial qualification assets to the strongest feasible evidence level, document blocked fixture classes honestly, and promote Windows security claims only if the evidence supports it. Remove internal plan-number implementation history from normative public docs and distinguish source support from published/prebuilt artifact support.

This plan also performs the final closure verification for Plans 120–125. It must not create a new optimization/security roadmap unless a release-blocking defect is actually demonstrated.

---

## Ordering and dependencies

Required order:

```text
121 lifecycle correctness
    ↓
122 wheel/CLI deduplication
    ↓
123 stock static-handler fast path
    ↓
124 runtime worker right-sizing
    ↓
125 Windows evidence + documentation + closure
```

Plans 123 and 124 may be developed in parallel after Plan 122 if their benchmarks use the same committed baseline and final measurements are rerun after both land.

Plan 125 should document the resulting packaging/runtime architecture, so it closes last.

---

## Global acceptance criteria

This roadmap is complete only when all of the following are true:

1. `wait_ready()` cannot return success from `Starting`, `Created`, `Draining`, `Stopped`, `Failed`, or a timed-out readiness future.
2. Readiness regression tests are deterministic and do not wait for the production 30-second timeout.
3. The Python wheel contains only one linked Rust server implementation; if the second standalone executable remains, there must be recorded evidence that removing it would break a supported capability and no low-complexity shared-native alternative exists.
4. `python -m eggserve`, an installed `eggserve` command, `serve_directory()`, and `ServerProcess` pass installed-wheel smoke tests from outside the source tree.
5. The standalone Cargo CLI remains available independently of the wheel.
6. Wheel/component sizes are recorded before and after Plan 122; no functionality is removed merely to improve size.
7. The stock `SimpleHTTPRequestHandler` path is either proven native-fast with compatibility tests or explicitly retained after benchmarks show the optimization is not worthwhile.
8. Arbitrary/custom Python handler subclasses continue to dispatch through Python and preserve documented behavior.
9. Python runtime thread/memory cost is measured for one and multiple servers; any worker-count change is justified by measurements and does not produce a meaningful representative throughput/latency regression.
10. No process-global Tokio runtime is introduced unless it demonstrably improves the measured case and has a simpler, fully tested lifecycle than the per-server design.
11. Windows security wording matches executed evidence. Unexecuted adversarial fixture classes are listed as unqualified rather than silently treated as passing.
12. Routine CI remains small; targeted Windows/deep qualification remains release/manual evidence unless an existing routine regression gap is demonstrated.
13. Public README/security/API docs describe current invariants and supported artifacts rather than Plan 108/109/other implementation history.
14. `cargo fmt`, workspace clippy/tests, TLS checks, and installed-wheel Python tests pass after the track.
15. No new protocol/server/framework capability, dependency framework, release automation, or permanent benchmark bureaucracy is added.

---

## Rejection conditions

Reject an implementation in this track if it does any of the following:

- treats a readiness timeout as success;
- fixes lifecycle behavior only in Python wrappers while leaving the native method semantically wrong;
- deletes the bundled binary but silently drops `eggserve`, `python -m eggserve`, or subprocess behavior;
- copies the full Rust CLI parser into Python merely to avoid an internal shared Rust module;
- fast-paths arbitrary subclasses based on fragile introspection assumptions;
- bypasses `SecureRoot`/`StaticResponder`/canonical response normalization for speed;
- adds a global runtime singleton without explicit shutdown/test semantics;
- adds a large CI matrix or makes adversarial suites mandatory per commit;
- promotes Windows to hardened/untrusted-public-content support without adversarial evidence;
- removes TLS, Python versions, HTTP semantics, or security defaults to reduce size;
- turns minor documentation wording into another numbered follow-up after Plan 125.

---

## Closure rule

Plans 000–119 are historical records and remain untouched except for forward links only if absolutely necessary. Plans 120–125 are the final bounded corrective/efficiency track authorized by this roadmap.

If implementation uncovers a genuinely release-blocking correctness/security defect, record it in the active plan that discovered it and correct it there when reasonably scoped. Do not create another roadmap simply because implementation details differ from the hypothesis above.
