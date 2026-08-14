# Plan 132 — Executable Examples and Product Demonstrations

## Status

**READY FOR HANDOFF — 2026-08-14.**

Governing roadmap: Plan 128.

Depends on: Plan 130 repository cleanup decisions and Plan 131 documentation hierarchy. Coordinate with Plan 133 for Rust public API usage.

The current repository has several Python examples, but they are skewed toward subprocess helpers and do not yet provide a balanced, executable demonstration of EggServe as CLI, Python `http.server` replacement, and Rust library.

This plan creates a small canonical example set. Examples must demonstrate actual supported behavior, be mechanically checked, and avoid becoming a second documentation system.

---

## Example design rules

Every example must satisfy all of these:

- uses supported public API only;
- has one clear purpose;
- is small enough to read without framework boilerplate;
- demonstrates a product surface that differs meaningfully from another example;
- does not weaken security defaults just to make the demo easier;
- does not depend on external Python/Rust packages unless the product itself requires them;
- can be compiled or smoke-run through a documented verification command;
- includes shutdown/cleanup behavior where it starts a server;
- does not use privileged ports;
- binds loopback unless the example is specifically documenting public bind opt-in;
- avoids sleeps as the primary readiness mechanism when the API exposes readiness.

Do not add a large example application, frontend, router, benchmark app, or tutorial framework.

---

## Track A — Add `examples/README.md` as the example index

Create a concise index that groups examples by surface:

```text
CLI
Python http.server facade
Python convenience/subprocess APIs
Rust static server
Rust custom service
Rust primitives
```

For each example, provide:

```text
what it demonstrates
how to run it
whether it blocks until Ctrl+C
which security defaults remain active
```

This file should link back to normative docs rather than restating compatibility/security policy.

---

## Track B — Canonical CLI demonstrations

Do not create shell scripts unless they add real value. Prefer documented command sequences in `examples/README.md` plus a reusable fixture directory if needed.

Required CLI demonstrations:

### Example B1 — Safe local static server

```sh
eggserve --directory ./examples/site
```

Demonstrate:

```text
loopback bind
index file serving
GET and HEAD
no directory listing by default
no dotfile serving by default
```

A tiny fixture tree may be added under `examples/site/` if it remains obviously non-production sample content, for example:

```text
examples/site/index.html
examples/site/assets/example.txt
examples/site/.hidden-example
```

Do not add large binary fixtures.

### Example B2 — Explicit public bind

Show the required opt-in:

```sh
eggserve --directory ./examples/site --public --port 8080
```

The text must explicitly state that this changes network exposure and does not itself provide edge TLS/proxy functionality.

### Example B3 — Opt-in directory listing

Show only if useful for parity with `python -m http.server`:

```sh
eggserve --directory ./examples/site --directory-listing
```

Make clear that listing is off by default.

### Acceptance criteria

- [ ] default CLI example works from the release binary or installed wheel console script;
- [ ] example fixture is tiny and text-only;
- [ ] public bind is explicit;
- [ ] no unsafe flag is presented as the default recommendation.

---

## Track C — Canonical Python static `http.server` replacement

Add or rename an example so the canonical Python library story is unmistakable, recommended name:

```text
examples/python_http_server_static.py
```

Required shape:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="examples/site")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    print(f"Serving on http://{server.server_address[0]}:{server.server_address[1]}")
    server.serve_forever()
```

Prefer port 8000 for copy/paste familiarity or port 0 for automated smoke mode. If the committed example blocks interactively on a fixed port, create test code that imports/reuses the handler/server construction with port 0 rather than requiring a human port.

The example must describe the intentional safe-default differences from stdlib in comments or linked docs, but not with a long embedded policy essay.

### Required live smoke

Using an installed wheel:

```text
start the example or equivalent imported construction on loopback port 0
GET index fixture
HEAD fixture
prove hidden file denied
shutdown
```

### Acceptance criteria

- [ ] example uses `eggserve.server`, not subprocess helpers;
- [ ] source shape closely resembles stdlib `http.server` usage;
- [ ] installed-wheel smoke succeeds;
- [ ] static fast path remains eligible for the exact stock handler configuration used by the example;
- [ ] hidden/listing/symlink safe defaults are not disabled.

---

## Track D — Canonical Python custom handler

Retain or replace `examples/simple_http_handler.py` with a complete custom-handler example, recommended name:

```text
examples/python_custom_handler.py
```

Required behavior:

```text
GET /health -> 200 text/plain or application/json
other path -> 404
explicit Content-Length
context-managed ThreadingHTTPServer
bounded/default response behavior
```

Use only synchronous handler methods. Do not pretend the API supports streaming response generation or coroutine handlers.

A short POST/body example is optional only if it demonstrates the bounded request-body facade cleanly and does not imply application-server scope.

### Acceptance criteria

- [ ] custom handler runs through the Rust-owned runtime;
- [ ] no raw socket access is used;
- [ ] no `translate_path()` workaround is used;
- [ ] response framing is compatible with runtime-owned headers;
- [ ] live smoke verifies the expected response and shutdown.

---

## Track E — Retain only distinct Python convenience examples

Review existing:

```text
examples/python_basic.py
examples/python_dynamic_static.py
examples/python_safe_download.py
examples/simple_http_handler.py
```

After adding the canonical facade examples, delete/merge examples that duplicate the same story.

Potential retained purposes:

- `python_subprocess.py` — demonstrate `ServerProcess` only;
- `python_safe_download.py` — retain if it uniquely demonstrates hardened primitive/static-response ownership without reopening translated paths;
- `python_dynamic_static.py` — retain only if it still demonstrates a supported low-level composition distinct from the custom-handler example.

Do not keep four examples merely because they already exist.

### Acceptance criteria

- [ ] every retained Python example has a distinct purpose;
- [ ] canonical facade examples are named/discoverable before advanced helpers;
- [ ] stale comments claiming no callback/custom system are corrected;
- [ ] subprocess API is clearly optional.

---

## Track F — Rust static server example

Add a Cargo-recognized example under the library crate, recommended path:

```text
crates/eggserve-core/examples/static_server.rs
```

Use public APIs only.

Required conceptual flow:

```rust
RuntimeConfig::builder()
    .bind("127.0.0.1:8000".parse()?)
    .build()?

Server::builder()
    .runtime(...)
    .static_service(root)
    .build()?

let handle = server.start().await?;
handle.ready().await?;
...
handle.shutdown().await?;
handle.wait().await?;
```

Adapt to the exact current API rather than changing the API to match this pseudocode unless Plan 133 identifies genuine ergonomic friction.

The example may use `#[tokio::main]` because Tokio is already a library dependency/runtime substrate; do not introduce another runtime.

The example should accept an optional directory argument using `std::env`, defaulting to `.` or the sample fixture. Do not add clap.

### Acceptance criteria

- [ ] `cargo check -p eggserve-core --example static_server` passes;
- [ ] example starts and serves a real file;
- [ ] no direct Hyper import;
- [ ] no internal EggServe module import;
- [ ] graceful shutdown path is demonstrated or documented;
- [ ] security policy remains safe-by-default.

---

## Track G — Rust custom service example

Add:

```text
crates/eggserve-core/examples/custom_service.rs
```

Use the public `Server`, `RuntimeConfig`, `service_fn`/`Service`, `Request`, and canonical response types.

Required behavior:

```text
GET /health -> 200 small body
GET / -> another small response or 404
unsupported/unmatched paths -> controlled response
```

The point is to demonstrate that downstream Rust projects can implement simple HTTP services on EggServe's transport without importing Hyper and without implying that EggServe is a high-level web framework.

Keep request routing deliberately trivial (a `match` on method/path is sufficient). Do not add a router abstraction.

### Acceptance criteria

- [ ] example compiles with `cargo check -p eggserve-core --example custom_service`;
- [ ] live request returns expected body/status;
- [ ] no direct Hyper API appears;
- [ ] no new router/middleware abstraction is introduced;
- [ ] example uses bounded/runtime-owned request/response semantics.

---

## Track H — Optional primitives-only Rust example

Add only if it demonstrates an otherwise non-obvious public library capability.

Candidate:

```text
crates/eggserve-core/examples/primitives.rs
```

Possible scope:

- construct/inspect canonical method/header/request/response types;
- demonstrate safe response normalization;
- demonstrate intended `primitives` facade without starting a socket.

Do not create this example if it would merely enumerate types without a useful task.

---

## Track I — Example smoke verification

Examples should not create a large new CI job.

Preferred verification split:

### Routine Rust

If `cargo test --workspace` does not compile examples, do not automatically add `--all-targets` to routine CI if that materially increases work. Instead add to `verify.sh full`:

```sh
cargo check -p eggserve-core --examples
```

### Python

Add a small installed-wheel example smoke path to `scripts/test-python-wheel.sh` or a `verify.sh full` helper only if it can run deterministically with loopback port 0 and clean shutdown.

At minimum:

```text
canonical Python static facade
canonical Python custom handler
```

### CLI

Reuse the existing real-fixture release smoke where practical rather than creating another process harness.

### Acceptance criteria

- [ ] canonical Rust examples are mechanically compiled;
- [ ] canonical Python examples or their shared construction are smoke-tested against the installed wheel;
- [ ] example verification does not require internet access;
- [ ] no benchmark or broad platform CI gate is added;
- [ ] server processes are always cleaned up.

---

## Documentation integration

README quickstarts should either use the exact canonical examples or link directly to them. Avoid maintaining conceptually identical but syntactically divergent snippets.

`examples/README.md` should be the index; deeper behavior remains owned by `docs/`.

---

## Rejection conditions

Reject an implementation that:

- adds a demo web framework/router;
- uses unsafe/public bind by default;
- adds third-party HTTP clients solely to test examples;
- adds large fixtures;
- demonstrates raw translated paths or raw sockets;
- leaves examples uncompiled/unexecuted;
- duplicates the same Python story across several files;
- treats subprocess helpers as the canonical `http.server` replacement;
- introduces another async runtime for Rust examples.

---

## Final acceptance criteria

Plan 132 is complete when:

- [ ] `examples/README.md` indexes the supported surfaces;
- [ ] CLI static demonstration exists and works;
- [ ] canonical Python static `http.server` replacement example exists and works from installed wheel;
- [ ] canonical Python custom handler example exists and works;
- [ ] redundant Python examples are merged/deleted;
- [ ] Rust static server example compiles and serves a fixture;
- [ ] Rust custom service example compiles and responds over TCP;
- [ ] optional primitives example is added only if it has distinct value;
- [ ] full/manual verification mechanically checks canonical examples;
- [ ] no application-framework scope is introduced.