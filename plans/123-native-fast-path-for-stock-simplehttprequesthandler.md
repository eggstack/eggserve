# Plan 123 — Native Fast Path for Stock SimpleHTTPRequestHandler

## Status

**READY FOR HANDOFF — 2026-08-11.**

Parent roadmap: Plan 120.
Depends on: Plans 121–122.

Reviewed baseline:

```text
main = bae3dce5f8be876a083434918cdfc974b9781c75
crates/eggserve-python/python/eggserve/server.py blob = 97a69822fb574a9fd680672f9bd7e448aa2b3f77
```

---

## Problem statement

The supported `http.server`-shaped static path currently configures a native `StaticResponder`, but still installs a Python callback for every request:

```text
Rust runtime
  -> PythonCallbackService
  -> Python HTTPServer._handle_request
  -> construct SimpleHTTPRequestHandler
  -> do_GET/do_HEAD
  -> Python _static_response
  -> native StaticResponder.respond
  -> Python response object
  -> Rust canonical response conversion
  -> transport
```

This retains security ownership in Rust, but the common stock static-server case pays for GIL acquisition, Python object construction, callback semaphore admission, and Rust↔Python response conversion even though request resolution/opening/streaming are native operations.

The most common documented form is narrow and identifiable:

```python
Handler = partial(SimpleHTTPRequestHandler, directory="public")
ThreadingHTTPServer(("127.0.0.1", 8000), Handler)
```

A native fast path can remove Python from this request hot path, but only when behavior can be proven equivalent. Arbitrary subclasses are part of the supported compatibility surface and must retain Python dispatch.

---

## Goal

For the exact stock `SimpleHTTPRequestHandler` configuration, serve static requests directly through EggServe's canonical Rust static service without entering Python per request.

Maintain a conservative rule:

> If handler semantics might depend on Python overrides or mutable class behavior, use the existing Python callback path.

This is a performance optimization, not a new static-serving implementation.

---

## Non-goals

Do not:

- bypass `SecureRoot`, `StaticResponder`, canonical response normalization, runtime limits, or opened-handle streaming;
- fast-path arbitrary subclasses by fragile introspection;
- implement Python bytecode/method-override analysis;
- remove subclass/custom-handler support;
- change stdlib-shaped constructor signatures;
- create a Python worker pool or new cache layer;
- add response/file caches;
- add MIME-sniffing or protocol features;
- add a permanent benchmark CI gate.

---

## Track A — Baseline the actual callback cost

Before implementing the fast path, benchmark the current installed-wheel compatibility path against the already-native static server path using the same content/root and equivalent safe policy.

At minimum measure:

```text
1 KiB file GET
64 KiB file GET
HEAD for the same files
conditional 304 path
range response
concurrency 1
representative moderate concurrency (e.g. 8/16/32)
```

Collect:

```text
requests/s or completed requests over a fixed interval
p50/p95 latency when the harness supports it
process CPU time/utilization
RSS
Python callback count or a direct proof that each request enters Python
```

Use a simple existing/local benchmark tool or a small repository script if necessary. Do not add a benchmark framework dependency. Run multiple samples and report median plus enough variability to distinguish signal from noise.

Implementation is justified if the stock Python path shows a repeatable material penalty, defined for this plan as at least one of:

- >=10% lower throughput on small/static requests;
- >=10% higher CPU per completed request;
- clearly measurable tail-latency inflation attributable to callback/GIL contention at moderate concurrency.

If no such difference exists, record the result and close this plan without retaining speculative fast-path code.

### Acceptance criteria

- baseline compares equivalent Rust-static and stock compatibility behavior;
- benchmark does not compare different security policies or logging modes;
- measurements are repeatable enough to support an implementation/no-implementation decision;
- no permanent CI benchmark is added.

---

## Track B — Define a conservative fast-path eligibility contract

The initial eligibility rule should be intentionally narrow.

Preferred eligible shape:

```text
RequestHandlerClass is exactly SimpleHTTPRequestHandler
OR functools.partial whose .func is exactly SimpleHTTPRequestHandler
```

and all request-relevant captured configuration is representable exactly by the native static server path.

Do **not** automatically fast-path subclasses merely because they appear not to override `do_GET`. Subclasses can affect behavior through:

```text
do_GET / do_HEAD
send_head
_static_response
guess_type
extensions_map
index_pages
directory_listing
follow_symlinks
allow_dotfiles
constructor behavior
class/instance hooks
```

For the first implementation, fall back to Python whenever exact equivalence is uncertain.

The common `partial(SimpleHTTPRequestHandler, directory=...)` form should remain eligible because the directory is immutable server configuration, not request-time Python behavior.

If exact stock class attributes have been mutated away from defaults before server construction and the native direct path cannot represent them exactly, fall back to Python rather than silently ignoring them.

### Acceptance criteria

- eligibility has a small deterministic implementation;
- exact stock handler with a `directory=` partial is eligible;
- arbitrary subclass is ineligible by default;
- unsupported/mutated static settings cause safe fallback, not approximate native behavior;
- no `inspect`-heavy heuristic attempts to prove arbitrary subclass equivalence.

---

## Track C — Route eligible stock handlers through the existing native static service

For eligible stock configuration, construct `_NativeServer` without a Python handler callback so the existing Rust `Server::start()` static path owns request handling directly.

Do not create a parallel responder pipeline. Reuse the production static service and its runtime-owned file-stream admission.

The current Python static compatibility path captures:

```text
root/directory
directory_listing
follow_symlinks
allow_dotfiles
index_pages
extensions_map
```

Determine which of these differ from native defaults and which must be passed into the native static configuration for exact stock behavior.

For the initial fast path, it is acceptable to restrict eligibility to configurations whose values exactly equal native defaults, plus the directory/root. If adding small native configuration parameters for `index_pages`/MIME metadata is simpler and already corresponds to core `ServeConfig`, that is allowed. Do not enlarge the low-level API merely to fast-path obscure subclass customization.

### Acceptance criteria

- eligible requests do not invoke `PythonCallbackService`;
- no GIL is acquired for normal request handling after server startup in the eligible static path;
- filesystem opening/resolution remains inside canonical native static service;
- runtime connection/file-stream limits remain unchanged;
- response normalization remains unchanged;
- static request-body rejection remains unchanged;
- no second Rust static responder implementation is introduced.

---

## Track D — Preserve fallback behavior exactly

The existing Python callback path remains authoritative for:

```text
BaseHTTPRequestHandler subclasses
SimpleHTTPRequestHandler subclasses
custom do_GET/do_HEAD
custom send_head
overridden guess_type or other request-sensitive behavior
configuration not exactly representable by the native fast path
```

Add tests using subclasses that visibly prove Python execution, for example:

- override `do_GET` to return custom bytes;
- override `guess_type` to return a sentinel MIME type;
- override `send_head` if it is part of the documented compatibility behavior;
- class attribute customization that intentionally makes a handler ineligible.

These tests must fail if eligibility accidentally bypasses Python.

### Acceptance criteria

- custom handler responses remain byte-for-byte/status/header correct under documented normalization;
- `guess_type` override behavior remains visible for eligible documented target cases via fallback;
- subclass logging/method behavior is not silently skipped;
- handler callback semaphore still limits Python-dispatched requests;
- fast-path and fallback selection is fixed at server configuration time rather than re-introspected per request.

---

## Track E — HTTP/security parity regression matrix

For stock fast-path vs the old compatibility semantics, test at minimum:

```text
GET direct file
HEAD direct file
index.html then index.htm ordering
missing file
path traversal rejection
dotfile denial
symlink/reparse denial according to platform policy
directory listing default denial
directory listing opt-in when eligible/representable
If-None-Match / If-Modified-Since
Range / If-Range
unknown extension MIME fallback
request body rejection
port 0 binding
TLS HTTPSServer stock static handler
```

Where a non-default configuration is intentionally not fast-path eligible, assert fallback rather than forcing native support.

### Acceptance criteria

- response status/headers/body semantics match the canonical Rust static service;
- HEAD never emits the body while preserving representation metadata;
- path confinement is identical to direct native serving;
- file stream permits remain runtime-owned and shared;
- HTTPS uses the same eligibility decision and rustls runtime;
- no security default changes.

---

## Track F — Measure the implemented result

Repeat Track A measurements after implementation using the installed wheel.

Record:

```text
case                      before   after   relative delta
1 KiB GET throughput
64 KiB GET throughput
HEAD throughput/latency
moderate concurrency CPU
RSS
```

Also directly prove that the stock path no longer constructs a Python handler per request. This can be done with a test-only counter/monkeypatch around the handler constructor or another narrow assertion; do not add production instrumentation solely for benchmarking.

If the fast path fails to produce the expected material improvement or meaningfully complicates the API, revert it and record a measurement-only closure.

### Acceptance criteria

- implemented optimization produces a repeatable material benefit beyond benchmark noise;
- no representative static case regresses materially;
- RSS does not increase meaningfully due to duplicated static configuration;
- benchmark artifacts remain lightweight/manual.

---

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHON=python3.14 bash scripts/test-python-wheel.sh
```

Run focused Python compatibility tests for exact-stock and subclass fallback behavior plus the manual before/after benchmark.

Routine CI job count remains unchanged.

---

## Explicit acceptance criteria

Plan 123 is complete when either the optimization path or the evidence-only path below is satisfied.

### Optimization closure

- [ ] baseline demonstrates material Python callback overhead;
- [ ] exact stock `SimpleHTTPRequestHandler` / directory partial has a deterministic native eligibility path;
- [ ] eligible static requests do not enter Python per request;
- [ ] arbitrary subclasses retain Python dispatch;
- [ ] mutated/unrepresentable configuration safely falls back;
- [ ] HTTP/security/TLS parity tests pass;
- [ ] installed-wheel benchmarks demonstrate a repeatable benefit;
- [ ] no new dependency/framework/cache is introduced;
- [ ] routine CI remains unchanged in shape.

### Evidence-only closure

If baseline overhead is below the materiality threshold:

- [ ] measurements are recorded;
- [ ] no speculative fast-path production code is retained;
- [ ] plan is marked complete with “no change justified”.

---

## Rejection conditions

Reject the implementation if it:

- fast-paths all `issubclass(SimpleHTTPRequestHandler, ...)` handlers;
- bypasses Python overrides silently;
- bypasses canonical Rust confinement/normalization for speed;
- adds a cache, alternate file server, or second responder;
- adds per-request reflection/introspection that offsets the optimization;
- changes MIME/index/security semantics merely to fit the native path;
- keeps complexity despite no repeatable performance benefit.
