# Plan 094 — Python `http.server` Fit and HTTP Correctness Roadmap

## Status

Proposed roadmap for the next bounded EggServe workstream.

This roadmap follows Plans 000–093 and does not reopen their retired CI, evidence, qualification, or release machinery. Routine CI remains the current two-job regression screen. Crates.io and PyPI release decisions remain manual maintainer actions.

## Goal

Complete EggServe's intended identity as:

> A hardened, HTTP-correct static file server with a Python-facing API that is source-familiar to users of the standard-library `http.server` module.

The current Rust core already provides most of the difficult security and transport foundations: constrained request-target parsing, capability-based filesystem resolution, bounded connections and file streams, HTTP/1 transport through Hyper, conditional requests, byte ranges, timeout enforcement, and canonical response normalization.

The remaining work is not a general server expansion. It is a focused reconciliation of three gaps:

1. Correct a small set of concrete HTTP semantics defects in the existing static-serving path.
2. Replace the bespoke primary Python server programming model with a narrow compatibility facade shaped like `http.server`.
3. Reconcile the public Python surface and tests around that final product contract.

## Product definition after this roadmap

EggServe will provide two closely related capabilities.

### Hardened static server

The CLI and built-in static handler continue to serve one configured filesystem root with safe defaults:

- loopback bind by default at the CLI level;
- GET and HEAD for the built-in static handler;
- no request bodies for the built-in static handler;
- no symlink following unless explicitly enabled;
- no dotfile serving unless explicitly enabled;
- no directory listing unless explicitly enabled;
- descriptor-relative or handle-relative filesystem access where supported;
- bounded connections and file streams;
- bounded header, body, handler, and connection deadlines;
- correct conditional and range response semantics;
- sanitized logging.

### Python `http.server`-shaped library

The supported Python server API will center on these documented names:

- `HTTPServer`
- `ThreadingHTTPServer`
- `HTTPSServer`
- `ThreadingHTTPSServer`
- `BaseHTTPRequestHandler`
- `SimpleHTTPRequestHandler`

The compatibility target is the documented public behavior of Python 3.14 `http.server`, not the private implementation details of `socketserver`, `http.server`, or CPython's socket objects.

The expected primary usage is:

```python
from functools import partial
from eggserve.server import ThreadingHTTPServer, SimpleHTTPRequestHandler

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Custom handlers use familiar `do_METHOD` dispatch and response methods:

```python
from eggserve.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = b"ok\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_error(404)

with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Rust continues to own sockets, HTTP parsing, timeout enforcement, response validation, file streaming, and graceful shutdown. The familiar Python API is a facade over that runtime, not a second HTTP implementation.

## Governing constraints

All phases must preserve these constraints.

1. EggServe remains a static file server and low-level HTTP server primitive, not an application framework.
2. No ASGI or WSGI adapter is added.
3. No routing framework, middleware stack, dependency injection, templates, sessions, authentication system, cookie framework, or plugin host is added.
4. No CGI implementation is added.
5. No upload/write API is added to `SimpleHTTPRequestHandler`.
6. No reverse proxy, forwarding proxy, cache proxy, or upstream client behavior is added.
7. No WebSocket, CONNECT, protocol upgrade, HTTP/2, HTTP/3, or trailer support is added.
8. No multipart byte-range implementation is added. Single-range behavior remains the supported static subset.
9. No compression, content negotiation framework, CORS feature set, or automatic ACME is added.
10. The experimental HTTP client is not expanded as part of this work.
11. The Rust filesystem confinement model is reused; the Python compatibility facade must not reconstruct and reopen paths.
12. Handler-generated responses always pass through canonical validation and normalization.
13. Unsafe stdlib behavior is not copied merely for behavioral identity.
14. Routine CI remains one workflow with the existing direct `rust` and `python` jobs.
15. No new required workflow, platform matrix, scheduled job, evidence registry, generated checklist, or hosted release gate is added.
16. Release remains manual.
17. Tests should be consolidated around the final contract rather than multiplied around old and new APIs simultaneously.
18. No plan may claim support beyond the platform evidence already established by the repository.

## Compatibility boundary

### Required server behavior

The Python facade must provide the following documented behaviors where they map cleanly onto the Rust runtime:

- server construction from `(host, port)` and a handler class;
- `serve_forever(poll_interval=...)`;
- `shutdown()`;
- `server_close()`;
- `handle_request()` for one-request test and embedding use;
- context manager support;
- `server_address`, `server_name`, and `server_port`;
- handler class construction and `do_METHOD` dispatch;
- `client_address`, `server`, `command`, `path`, `request_version`, and `headers`;
- `send_response()`, `send_response_only()`, `send_header()`, `end_headers()`, and `send_error()`;
- `log_request()`, `log_error()`, and `log_message()` override points;
- `date_time_string()` and `version_string()` helpers;
- bounded `rfile` and `wfile` objects with the common file-like operations needed by ordinary handlers;
- `SimpleHTTPRequestHandler(directory=...)`;
- `index_pages` containing `index.html` and `index.htm` by default;
- safe directory redirects, optional listings, MIME guessing, conditional requests, HEAD parity, and range responses;
- TLS classes backed by the existing Rust TLS implementation.

### Intentionally unsupported compatibility details

The facade does not promise:

- inheritance from `socketserver.BaseServer` or its mixins;
- access to a raw Python socket object for the accepted connection;
- direct calls to `setup()`, `finish()`, `handle_one_request()`, or private parser methods as stable extension points;
- mutation of Hyper connection state;
- raw response-line or raw header injection;
- undocumented CPython internals;
- exact logging text, reason-phrase formatting, thread naming, or implementation-specific exception timing;
- unlimited buffering in `rfile` or `wfile`;
- compatibility with code that depends on insecure symlink, dotfile, directory-listing, or header behavior.

These differences must be documented explicitly rather than hidden.

## Security policy versus stdlib behavior

API familiarity does not override EggServe's security invariants.

The compatibility facade keeps these safe defaults:

- dotfiles denied;
- symlinks denied;
- directory listing denied;
- validated response status and headers;
- bounded callback concurrency;
- bounded request bodies;
- bounded handler response buffering;
- no bodies on statuses that prohibit content;
- Rust-owned response framing;
- no handler-controlled hop-by-hop fields.

A caller that explicitly passes a wildcard address such as `("", 8000)`, `("0.0.0.0", 8000)`, or `("::", 8000)` has already supplied an explicit public bind address through the standard constructor shape. The Python facade should not require a second `public=True` acknowledgment. The CLI remains loopback-safe by default and retains its explicit `--public` guard.

## Current baseline requiring correction

The implementing agent must verify the current code rather than relying only on this roadmap, but the present baseline includes these known issues:

1. The primary Python server model is `Server(handler=request_to_response_callback)` rather than `HTTPServer(..., HandlerClass)`.
2. Request and response headers at the callback boundary use dictionaries and collapse duplicate fields.
3. File-backed callback response conversion does not preserve all body-source variants correctly.
4. `StatusCode::new()` accepts codes above 599.
5. `205 Reset Content` is not treated as body-forbidden.
6. `If-Range` uses weak entity-tag comparison and can accept weak validators.
7. Final origin responses do not consistently receive a `Date` field.
8. Generated directory-listing HEAD responses do not preserve the equivalent GET representation length.
9. Static directory index behavior checks only `index.html`, not the standard pair `index.html` and `index.htm`.
10. The current top-level Python API exposes multiple overlapping policy, body, method, request, response, server, and client type systems.

The plans below resolve these items in dependency order.

## Execution sequence

### Plan 095 — RFC 9110 static response corrections

Correct existing protocol defects before building the compatibility facade on top of them.

Required outcomes:

- status codes limited to 100–599;
- 205 response content suppressed;
- strong `If-Range` semantics;
- malformed or weak `If-Range` falls back to full 200;
- centrally generated `Date` for applicable origin responses;
- directory-listing HEAD metadata matches GET;
- focused Rust and live-wire regressions.

### Plan 096 — Base `http.server` compatibility facade

Implement the narrow Python server and base-handler model over the existing native runtime.

Required outcomes:

- `HTTPServer` and `ThreadingHTTPServer`;
- `BaseHTTPRequestHandler`;
- documented lifecycle and server attributes;
- `do_METHOD` dispatch;
- ordered duplicate-preserving headers;
- bounded request and response file-like objects;
- canonical response validation;
- no second accept loop or Python HTTP parser.

### Plan 097 — `SimpleHTTPRequestHandler` secure static integration

Map the familiar static handler onto the existing SecureRoot and file-streaming implementation.

Required outcomes:

- `directory=` constructor behavior;
- `index_pages = ("index.html", "index.htm")`;
- GET/HEAD parity;
- trailing-slash directory redirects;
- optional safe directory listings;
- MIME override point;
- conditional and single-range responses;
- resolver-opened file streaming without Python buffering;
- explicit safe-default divergences from stdlib.

### Plan 098 — TLS, public API reconciliation, and closure

Finish the standard-library-shaped surface and remove conflicting primary APIs without creating a compatibility framework of its own.

Required outcomes:

- `HTTPSServer` and `ThreadingHTTPSServer` as thin facades over existing Rust TLS;
- a curated `eggserve.server` public module;
- low-level primitives moved behind an explicitly advanced namespace;
- the bespoke native `Server` callback type demoted to an internal implementation detail;
- no experimental HTTP client exports from the default Python package;
- subprocess convenience retained only where it remains simple and non-conflicting;
- documentation and examples rewritten around the final API;
- redundant tests removed while retaining wire, filesystem, compatibility, and package coverage;
- final same-commit local and hosted verification.

## Dependency order and stop conditions

The plans are strictly ordered:

```text
095 -> 096 -> 097 -> 098
```

Do not begin Plan 096 until Plan 095's protocol corrections pass targeted wire tests.

Do not begin Plan 097 until the base handler can generate a validated in-memory response and lifecycle shutdown is reliable.

Do not begin public API cleanup in Plan 098 until the replacement API passes installed-wheel tests.

Stop and write a narrow corrective plan rather than continuing if implementation demonstrates any of the following:

- the compatibility facade requires a second HTTP parser;
- raw Python socket ownership is required;
- static files must be reopened by path after policy resolution;
- TLS requires a separate serving architecture;
- handler semantics require ASGI/WSGI, routing, or middleware;
- the work requires expanding routine CI;
- a documented compatibility behavior cannot be implemented without weakening an existing security invariant.

## Testing policy

This work should simplify the verification surface.

### Required retained layers

1. Focused Rust unit tests for planner and canonical-response rules.
2. Raw HTTP wire tests for externally observable status, header, framing, HEAD, conditional, range, and Date behavior.
3. Filesystem confinement tests for traversal, symlink, dotfile, and directory behavior.
4. Installed-wheel Python compatibility tests for documented server and handler behavior.
5. Small package/CLI smoke tests.

### Avoid

- duplicating every Rust planner test in Python;
- snapshotting the entire exported namespace when a small explicit API list is enough;
- timing-heavy tests where events or barriers can provide deterministic synchronization;
- tests for private CPython implementation details;
- adding a new compatibility corpus framework;
- broad browser matrices;
- new fuzz targets unless a parser change introduces a genuinely new input grammar;
- keeping tests solely for APIs removed from the supported surface.

Existing valuable tests may be retained. Redundant tests should be deleted as old public APIs are demoted or removed.

## Documentation deliverables

By roadmap completion, active documentation must clearly state:

- the supported `eggserve.server` API;
- the exact compatibility boundary with `http.server`;
- safe-default differences;
- the supported HTTP version and static subset;
- Linux, macOS, and Windows confinement qualifications;
- TLS limitations;
- the advanced/low-level namespace contract;
- explicit non-goals;
- manual release policy;
- simple local verification commands.

Historical plan files remain historical and do not need terminology rewrites unless they are linked as current guidance.

## Completion criteria

This roadmap is complete only when all of the following are true on one final commit:

1. The identified RFC defects are corrected and covered by targeted wire tests.
2. `from eggserve.server import HTTPServer, ThreadingHTTPServer, HTTPSServer, ThreadingHTTPSServer, BaseHTTPRequestHandler, SimpleHTTPRequestHandler` succeeds from an installed wheel.
3. Familiar subclass-based `do_GET()` usage works without a separate application framework.
4. `SimpleHTTPRequestHandler` serves full files, HEAD, conditional responses, and single ranges through the Rust static-serving path.
5. Directory redirects, index selection, and optional listings behave as documented.
6. Handler responses preserve duplicate headers and reject invalid framing or injection attempts.
7. File-backed static responses remain streamed by Rust and do not pass through Python memory.
8. The old bespoke native `Server` type is no longer the recommended or top-level primary API.
9. The default Python package no longer advertises the experimental HTTP client.
10. Active docs and examples use the final API and contain no contradictory constructor examples.
11. The final test inventory is smaller or no larger unless each added test covers a previously untested final-contract behavior.
12. `./scripts/verify.sh fast` passes.
13. `./scripts/verify.sh full` passes.
14. Both existing routine CI jobs pass on the final commit.
15. No new routine workflow, release automation, evidence system, or scope-expanding dependency was introduced.

## Handoff summary

Implementation should favor direct adaptation over abstraction:

- fix protocol semantics centrally;
- add one Python compatibility facade;
- reuse the existing Rust runtime and static responder;
- retain one canonical response path;
- delete superseded surface area and tests;
- stop when the documented `http.server`-shaped static server contract is complete.

Do not use this roadmap as authorization for broader application-serving, client, proxy, or edge-server work.