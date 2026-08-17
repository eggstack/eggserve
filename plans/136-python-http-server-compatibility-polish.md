# Plan 136 — Python `http.server` Compatibility Polish

## Status

**COMPLETE — 2026-08-17.**

Reviewed baseline:

```text
f3d0332d7334829e2aedc4a50e4a2b566ce89eae
```

Relevant completed work:

```text
Plans 094–101  HTTP server/API correctness and closure
Plan 123       stock SimpleHTTPRequestHandler native fast path
Plan 131       documentation and compatibility-contract polish
Plan 135       CLI regression correction and post-audit requalification
```

This is a **narrow compatibility-polish pass**, not a new server roadmap. The existing EggServe static server, Python six-class facade, Rust service boundary, filesystem confinement model, HTTP/1.1 runtime, and release/verification model are already established. The purpose of this plan is to close the remaining high-value differences from current Python `http.server` behavior that fit EggServe's existing product scope, while making intentional differences explicit rather than silently emulating unsupported `socketserver` internals.

At planning time, the comparison reference is the official Python 3.14 documentation plus the additive `http.server` surface documented for Python 3.15.0b4. Before implementation, re-check the current official 3.15 documentation/source only for the specific additive items named below. Do not turn this plan into a moving-target effort to copy every CPython implementation detail.

Primary references:

```text
https://docs.python.org/3.14/library/http.server.html
https://docs.python.org/3.15/library/http.server.html
https://github.com/python/cpython/blob/3.15/Lib/http/server.py
```

---

## Goal

Close the remaining useful `http.server` compatibility gaps without weakening EggServe's security or runtime ownership model.

Required outcomes:

1. add the small Python 3.15 static-serving configuration surface that naturally fits EggServe: `default_content_type`, `extra_response_headers`, CLI `--content-type`, and repeatable `-H`/`--header`;
2. make `BaseHTTPRequestHandler.send_error()` materially source-compatible for status/body/error-template semantics while Rust retains framing and transport ownership;
3. enrich the request-header facade with common read-only `HTTPMessage`-style operations used by real handlers;
4. eliminate the misleading implication that changing `protocol_version` can reconfigure EggServe's transport;
5. add only low-risk response/helper compatibility methods that do not restore raw socket or filesystem authority;
6. reconcile the CLI bind/TLS/documentation mismatches found in the post-closure review;
7. update compatibility documentation so implemented directory redirects, TLS support, HTTP/1.1-only semantics, and the new additive surface are represented truthfully;
8. prove the changes with focused existing tests and the existing verification pipeline, without adding CI/release machinery.

When this plan is complete, the Python `http.server` replacement track should return to ordinary maintenance. Future work should require a concrete compatibility bug or a clearly in-scope additive stdlib feature, not another broad parity sweep.

---

## Non-negotiable scope boundaries

Preserve all of the following.

### Product/runtime boundaries

- Keep the supported Python server facade limited to:
  - `HTTPServer`;
  - `ThreadingHTTPServer`;
  - `HTTPSServer`;
  - `ThreadingHTTPSServer`;
  - `BaseHTTPRequestHandler`;
  - `SimpleHTTPRequestHandler`.
- Remain HTTP/1.1 only as an EggServe server contract.
- Do **not** add HTTP/1.0 runtime mode solely to copy Python's CLI `--protocol` option.
- Do **not** add HTTP/2 or HTTP/3.
- Do **not** add ASGI, WSGI, CGI, routing, middleware, reverse proxying, WebSockets, CONNECT tunneling, upgrade handling, or application-framework behavior.
- Do **not** add a general `socketserver` replacement.
- Do **not** expose accepted sockets, listener file descriptors, raw TLS sockets, or arbitrary `ssl.SSLContext` objects to Python.
- Do **not** implement one-request `handle_request()` mode.
- Do **not** restore authoritative `translate_path()` or raw host paths to `list_directory()`.
- Do **not** add async Python handlers or unbounded Python response streaming.

### Security/ownership boundaries

- Rust remains authoritative for socket ownership, request parsing, path confinement, file opening, file streaming, response normalization, `Date`, `Content-Length`, connection persistence, and hop-by-hop/framing headers.
- Static roots remain validated and pinned at server construction.
- Safe static defaults remain: loopback bind, no directory listing, no symlinks, no dotfiles.
- Extra user headers must not weaken CR/LF/NUL validation, hop-by-hop rejection, content-length correctness, `nosniff`, range validators, or canonical framing.
- Python metadata hooks must not cause a second filesystem lookup, Python file open, path reopen, or GIL acquisition on the stock native static fast path.
- Do not weaken the CLI wildcard-bind acknowledgement rule.

### Project-complexity boundaries

- Do not add a parser framework such as `clap` for these CLI additions.
- Do not add a new dependency solely for compatibility helpers, MIME configuration, header parsing, DNS resolution, TLS argument handling, or tests.
- Do not add new CI workflows, test harnesses, evidence registries, or release automation.
- Keep release cadence manual.
- Extend existing test files and configuration types instead of creating parallel abstractions.
- Prefer one shared static-response configuration path so CLI, Python fast path, and Rust static service do not drift.

---

## Compatibility decisions fixed by this plan

Several reviewed differences are intentional and should be decided now rather than left ambiguous for implementers.

### HTTP protocol selection

EggServe remains HTTP/1.1 only. Python's `-p/--protocol` option and mutable `BaseHTTPRequestHandler.protocol_version` semantics do not justify adding an HTTP/1.0 mode.

Required policy:

```text
EggServe runtime protocol: HTTP/1.1
Python handler protocol_version: compatibility metadata constrained to HTTP/1.1
CLI --protocol: not added
```

A handler class that explicitly sets an incompatible `protocol_version` must fail clearly at server construction rather than appearing to work while being ignored.

### CGI

Do not add `CGIHTTPRequestHandler` or `--cgi`. CGI is outside EggServe's security/scope model and is removed from the Python 3.15 `http.server` direction.

### TLS password files

Do not add `--tls-password-file` or encrypted private-key password handling under this plan. That introduces secret-input and key-decryption behavior not needed for the intended local/hardened static-server scope. Combined certificate/key PEM support is separately addressed below because it already matches the Python `HTTPSServer` facade and requires no password channel.

### Deep `socketserver` hooks

Keep `fileno()`, one-request mode, raw socket ownership, arbitrary request-loop hooks, and thread-per-connection internals intentionally unavailable. The compatibility goal is useful handler/source familiarity, not implementation identity with `socketserver.TCPServer`.

---

## Execution order

Implement in this order:

```text
A. shared static metadata configuration
B. Python 3.15 SimpleHTTPRequestHandler additions
C. BaseHTTPRequestHandler/send_error/header-view polish
D. protocol/helper truthfulness
E. CLI bind and TLS consistency
F. focused regression/parity tests
G. active documentation reconciliation
H. existing verification and closure record
```

Tracks A and B should land together or in directly adjacent commits because the Python fast path and CLI must consume the same static configuration rather than inventing parallel behavior.

---

# Track A — Add one bounded static metadata configuration path

## Objective

Create the minimum native/static-service configuration needed for the additive Python 3.15 behavior without moving response construction back into Python.

The new configuration must support:

```text
default content type
extra response headers for static HTTP 200 responses
```

These are representation metadata, not filesystem policy and not transport framing.

## A1. Default content type

Today EggServe falls back to `application/octet-stream` when MIME detection does not produce a type. Preserve that as the default while allowing an explicit validated override.

Preferred ownership:

- represent the value in the static-service/static-response configuration used by the CLI and Python static facade;
- thread it into MIME selection/planning once;
- do not put MIME fallback policy in `RuntimeConfig`, because it is a service concern rather than transport configuration;
- do not change the low-level default for callers that do not specify an override.

Required validation:

- input must be a string at Python boundaries;
- reject CR, LF, and NUL;
- reject an empty value unless the existing MIME/value validator already gives a stronger controlled error;
- do not attempt to parse or normalize arbitrary MIME parameters beyond existing project policy;
- invalid values fail before serving or produce the existing generic fail-closed result at the narrowest appropriate boundary.

## A2. Extra static response headers

Represent extra headers as an ordered duplicate-preserving sequence:

```text
[(name, value), ...]
```

Do not reduce them to a map.

The configuration is applied only to final static responses with status `200 OK`, matching Python 3.15's `extra_response_headers` contract. It must **not** automatically apply to:

```text
206 Partial Content
304 Not Modified
301 directory redirect
4xx/5xx responses
informational responses
```

Apply the same policy to all EggServe-produced static `200` representations where applicable:

- direct files;
- native-selected index files;
- directory listings when listings are explicitly enabled.

## A3. Header collision and ownership rules

Python 3.15 documents that automatically generated headers are not overwritten by extra response headers. EggServe generates a larger correctness/security set than CPython, so define a deterministic ownership rule.

At minimum, extra headers must never override or inject:

```text
Connection
Keep-Alive
Proxy-Connection
Transfer-Encoding
Upgrade
Trailer
Content-Length
Date
Server, if runtime-configured
Content-Type
Content-Range
Accept-Ranges
ETag
Last-Modified
X-Content-Type-Options
```

Also preserve any other existing canonical/runtime-owned header list.

Recommended behavior:

- reject hop-by-hop/framing fields as invalid configuration;
- for representation headers that EggServe generates automatically, preserve the EggServe-generated value and do not replace it with an extra header;
- preserve ordered duplicates for safe non-owned fields such as repeated `Set-Cookie`-like or custom extension headers, even though EggServe itself does not implement a cookie abstraction;
- validate every name/value with the existing canonical header validators before activation;
- never partially apply an extra-header list after a later element fails validation.

If the existing response normalizer already owns a stronger reserved-header policy, reuse it rather than creating a second list.

## A4. Preserve the native fast path

The stock `SimpleHTTPRequestHandler` fast path must remain native when the selected behavior can be represented entirely by static configuration.

After this track, the fast-path eligibility model should be able to represent:

```python
SimpleHTTPRequestHandler
partial(SimpleHTTPRequestHandler, directory="public")
partial(SimpleHTTPRequestHandler, directory="public", extra_response_headers=[...])
```

provided no subclass/custom hook requires Python dispatch.

`default_content_type` configured through the exact stock class/default-safe configuration should also remain native.

Do not acquire the GIL per request merely because an immutable metadata option was supplied at server construction.

If a subclass overrides `guess_type()`, `do_GET()`, `do_HEAD()`, or other callback behavior, keep the existing callback fallback contract.

## A5. Rust API scope

Do not promote a broad generic header middleware system.

If the Rust static service requires new builder methods, keep them specific and bounded, for example conceptually:

```rust
StaticService::builder(root)
    .default_content_type(...)
    .extra_response_headers(...)
```

or an equivalent static-service configuration object already owned by the crate.

Avoid making these transport-wide `RuntimeConfig` knobs.

## Track A acceptance criteria

- [ ] one static-service configuration path owns default MIME fallback and extra successful-200 headers;
- [ ] default behavior remains byte-for-byte/semantically unchanged when no new option is supplied;
- [ ] safe extra headers preserve order and duplicates;
- [ ] invalid or runtime-owned headers cannot corrupt framing or security metadata;
- [ ] extra headers apply only to status 200;
- [ ] direct files, index files, and listings use the same policy;
- [ ] native static fast-path requests do not acquire the GIL solely for the new metadata knobs;
- [ ] no middleware/router/header-framework abstraction is introduced.

---

# Track B — Add the useful Python 3.15 `SimpleHTTPRequestHandler` surface

## Objective

Match the additive Python 3.15 static-serving API that fits EggServe without importing unrelated stdlib internals.

## B1. `default_content_type`

Add:

```python
class SimpleHTTPRequestHandler(BaseHTTPRequestHandler):
    default_content_type = "application/octet-stream"
```

Update `guess_type()` so unknown suffixes fall back to `self.default_content_type` rather than a hard-coded literal.

Requirements:

- subclass overrides work on the callback path;
- exact-stock/native configuration captures the default or explicit supported override at server configuration time;
- invalid values fail closed under Track A validation;
- GET, HEAD, index, and range metadata remain internally consistent;
- the existing `extensions_map` and `mimetypes` behavior remains first in precedence.

## B2. `extra_response_headers`

Update constructor compatibility from conceptually:

```python
SimpleHTTPRequestHandler(request, client_address, server, directory=None)
```

to:

```python
SimpleHTTPRequestHandler(
    request,
    client_address,
    server,
    *,
    directory=None,
    extra_response_headers=None,
)
```

Preserve source-familiar use through `functools.partial`.

Capture static configuration at server construction. A request must not be allowed to swap the root or mutate native response policy after activation.

Required behavior:

- `None` means no extra headers;
- a sequence of `(name, value)` pairs is accepted;
- malformed sequence structure fails clearly;
- names and values are strings;
- validation occurs atomically;
- safe duplicates remain ordered;
- runtime-owned/unsafe headers follow Track A rules;
- only 200 responses receive the extras.

Do not expose raw response serialization to Python.

## B3. Native fast-path eligibility

Update `_check_native_fast_path()` and `_static_handler_config()` together.

Allowed exact-stock partial keywords should become the smallest set necessary to support the stdlib-shaped constructor:

```text
directory
extra_response_headers
```

Do not silently ignore positional `partial.args` or unknown keywords.

A subclass remains callback-backed even when it only changes class attributes. This preserves the existing simple, auditable eligibility rule unless there is already a tested, strictly equivalent subclass fast-path mechanism.

## B4. Type stubs and public docs

Update:

```text
crates/eggserve-python/python/eggserve/server.pyi
crates/eggserve-python/python/eggserve/__init__.pyi, if relevant
crates/eggserve-python/python/eggserve/_native.pyi, only if the native config surface changes
```

Do not expose internal bridge classes merely to make typing easier.

## Track B acceptance criteria

- [ ] `default_content_type` exists and controls unknown-extension fallback;
- [ ] `extra_response_headers` is accepted with Python 3.15-shaped constructor semantics;
- [ ] safe extra headers appear on static 200 GET and equivalent HEAD metadata;
- [ ] extras do not appear on 206/304/301/error responses;
- [ ] automatic EggServe headers cannot be overwritten;
- [ ] exact-stock configuration remains native/GIL-free per request;
- [ ] subclasses and custom hooks still use the bounded callback path;
- [ ] stubs match runtime signatures.

---

# Track C — Improve `BaseHTTPRequestHandler` compatibility where it is high value

## Objective

Make common custom handlers behave more like stdlib `BaseHTTPRequestHandler` without granting Python transport authority.

## C1. Correct `send_error()` semantics

The current simplified error response is insufficiently compatible with stdlib customization hooks. Rework it around the existing class attributes:

```text
responses
error_message_format
error_content_type
```

Required logical behavior:

1. obtain the short and long descriptions from `responses` when available;
2. use the caller's `message=` when supplied;
3. use the caller's `explain=` when supplied;
4. otherwise fall back to the standard short/long descriptions;
5. render `error_message_format` with the documented `code`, `message`, and `explain` fields;
6. HTML-escape message/explanation before interpolation when the configured template is the standard HTML-style representation;
7. encode deterministically as UTF-8 with a controlled replacement policy;
8. use `error_content_type` for generated error entities;
9. suppress the entity body for `HEAD` while retaining representation metadata as appropriate;
10. suppress generated entity bodies for statuses where HTTP semantics prohibit one, including informational 1xx, 204, 205, and 304;
11. keep status/header/body conversion atomic and subject to the existing maximum handler-response size;
12. malformed templates or handler customization failures must use the existing generic 500 fail-closed path without leaking exception text.

Do not restore Python ownership of `Connection`, `Content-Length`, or transport serialization solely for byte-for-byte CPython implementation parity. Rust remains the final authority for those fields. The compatibility target here is logical error status/body/customization behavior.

## C2. Preserve bounded response behavior

`send_error()` must continue using the bounded `wfile` facade and `max_handler_response_bytes`.

A pathological custom `error_message_format`, `message`, or `explain` must not permit an unbounded response allocation.

Reuse the existing `_BodyWriter` ceiling and fail closed if the generated body exceeds it.

## C3. Request header facade expansion

Extend `_HTTPMessage` as a **read-only** compatibility adapter. Do not replace Rust request parsing with Python `http.client.parse_headers()` and do not make the object mutable.

High-value operations to add where their semantics can be matched deterministically:

```text
__getitem__
keys()
values()
get_all()                # already present; retain duplicate behavior
items()                  # already present
raw_items()              # if useful and semantically identical to stored pairs
get_content_type()
get_content_maintype()
get_content_subtype()
get_content_charset()
get_param()
get_params()             # only if implemented through a small well-tested helper
```

Priority is compatibility with typical handler code such as:

```python
length = self.headers["Content-Length"]
ctype = self.headers.get_content_type()
charset = self.headers.get_content_charset()
for name, value in self.headers.items():
    ...
```

Requirements:

- header names remain case-insensitive;
- duplicate fields remain available through `get_all()` and ordered `items()`;
- helper parsing consumes only already-validated header values;
- no mutation methods are required;
- do not claim full `email.message.Message` parity;
- unsupported mutation or obscure MIME-message operations may remain absent and should be documented as outside the bounded adapter contract.

Where practical, differential unit tests against stdlib `HTTPMessage`/`Message` should be used for the specific helpers implemented, without adding a runtime dependency or a CI version matrix.

## Track C acceptance criteria

- [ ] `send_error()` honors `responses`, `error_message_format`, `error_content_type`, `message`, and `explain` in the supported bounded contract;
- [ ] HEAD and body-forbidden statuses do not emit a response entity;
- [ ] malformed/custom error rendering fails closed;
- [ ] normal `send_error()` output remains under the handler-response limit;
- [ ] common mapping/header helper operations work on `self.headers`;
- [ ] duplicate header access remains correct;
- [ ] the header object remains read-only and parser authority remains in Rust;
- [ ] no raw framing headers become Python-controlled.

---

# Track D — Make protocol and response helper behavior truthful

## Objective

Remove silent compatibility traps where a familiar stdlib attribute exists but cannot actually control EggServe's native runtime.

## D1. `protocol_version`

Keep:

```python
BaseHTTPRequestHandler.protocol_version = "HTTP/1.1"
```

but validate it when configuring a compatibility server.

Required behavior:

- exact `"HTTP/1.1"` is accepted;
- a handler class/subclass that sets `"HTTP/1.0"`, `"HTTP/0.9"`, or another value fails before activation with a clear compatibility/configuration error;
- do not silently ignore the value;
- do not add a second protocol selector in the native runtime;
- document that incoming request version metadata remains available through `request_version`, but EggServe's response/runtime contract is HTTP/1.1.

If the runtime already has a more precise accepted-request-version contract, preserve it; this track concerns the handler's advertised/output protocol configuration, not parser acceptance.

## D2. `log_date_time_string()`

Add the inexpensive stdlib-shaped helper:

```python
log_date_time_string()
```

with deterministic stdlib-compatible formatting.

This helper is pure formatting and does not conflict with native logging ownership.

## D3. Existing response metadata hooks

Audit these existing attributes/methods:

```text
server_version
sys_version
version_string()
date_time_string()
address_string()
log_request()
log_error()
log_message()
```

Required outcome is **truthfulness**, not necessarily wire-level CPython identity.

- Preserve `address_string()` as IP/string output without reverse DNS.
- Preserve subclass override calls where the compatibility facade already invokes them.
- Do not add a default fingerprinting `Server` header merely because CPython does.
- Do not let Python override the Rust-generated `Date` field.
- If `server_version`/`version_string()` do not affect the wire by design, state that explicitly in the compatibility documentation rather than implying otherwise.
- Keep default logging bounded/sanitized and avoid duplicating native operational logs.

Do not add a new logging framework or expose unsanitized request data.

## Track D acceptance criteria

- [ ] unsupported `protocol_version` values fail clearly instead of being ignored;
- [ ] HTTP/1.1 remains the only advertised EggServe server protocol;
- [ ] no CLI `--protocol` mode is added;
- [ ] `log_date_time_string()` is available and tested;
- [ ] response/logging helper documentation accurately distinguishes formatting/customization hooks from Rust-owned wire metadata;
- [ ] no new server fingerprint is emitted by default.

---

# Track E — Reconcile CLI bind and TLS behavior without broadening the runtime

## Objective

Correct the two remaining CLI truthfulness/compatibility mismatches found during review while keeping the core runtime narrow.

## E1. `--bind` hostname support

The active CLI documentation describes `--bind HOST[:PORT]`, while `args.rs` currently accepts only an `IpAddr` or `SocketAddr`. A normal invocation such as:

```sh
eggserve --bind localhost 8000
```

should either be supported or the CLI must stop claiming `HOST` support.

This plan chooses **support**, because hostname binds are conventional `http.server` behavior and can be implemented at startup without changing the stable Rust runtime API.

Preferred implementation constraints:

- keep `RuntimeConfig.bind` as `SocketAddr`;
- retain an unresolved CLI-only bind specification until startup resolution;
- resolve hostnames once before constructing/starting the runtime, using the standard library or existing Tokio facilities;
- do not add a DNS dependency;
- do not add dynamic re-resolution after startup;
- do not bind multiple listeners;
- use the resolver's first usable address under a documented deterministic policy;
- after resolution, re-run the existing unspecified-address/public-intent guard so a resolved wildcard cannot bypass `--public`;
- preserve literal IPv4 and bracketed IPv6 behavior;
- preserve port-slot semantics from Plan 135;
- resolution failure returns a controlled startup/configuration error, not a panic.

If implementation reveals that supporting hostnames would require changing the public `eggserve-core::server::RuntimeConfig` contract or adding multi-listener behavior, stop that subtrack and instead correct every CLI document/help string from `HOST` to `IP`. Do not widen the Rust runtime API solely for CLI hostname parity.

## E2. Combined PEM TLS CLI behavior

The Python `HTTPSServer` facade already supports:

```python
HTTPSServer(..., certfile="server.pem", keyfile=None)
```

where the same PEM may contain the certificate chain and private key. The CLI currently requires both `--tls-cert` and `--tls-key`.

Align the feature-gated CLI with the existing facade:

```text
--tls-cert PATH            enable TLS
--tls-key PATH             optional; default to --tls-cert path
```

Required rules:

- `--tls-key` without `--tls-cert` remains an error;
- `--tls-cert` without `--tls-key` uses the cert path as the key path;
- explicit separate key path remains supported;
- encrypted keys/password files remain unsupported under this plan;
- TLS remains rustls-owned;
- no arbitrary Python/OpenSSL `SSLContext` behavior is introduced.

## E3. CLI Python-3.15 static options

Expose the Track A/B metadata options through the existing manual parser:

```text
--content-type CONTENT_TYPE
-H NAME VALUE
--header NAME VALUE
```

`-H`/`--header` must be repeatable and preserve input order.

The CLI must feed the same static metadata configuration used by the Python facade. Do not build CLI-only header mutation after response planning.

Help output and `docs/cli.md` must identify that extra headers apply only to static `200 OK` responses and cannot replace runtime/representation-owned fields.

## E4. Do not overextend TLS parity

Explicitly leave these unavailable:

```text
--tls-password-file
encrypted private-key passwords
client certificates
SNI multi-certificate selection
certificate reload/ACME
HTTP/2 ALPN
```

## Track E acceptance criteria

- [ ] `--bind localhost` works without changing the stable core bind API, or all user-facing text is corrected to IP-only if the bounded implementation condition cannot be met;
- [ ] wildcard/public-intent protection remains effective after any hostname resolution;
- [ ] Plan 135 positional-slot behavior remains unchanged;
- [ ] TLS CLI accepts a combined cert/key PEM through `--tls-cert` alone;
- [ ] `--tls-key` without a cert remains rejected;
- [ ] `--content-type` reaches the shared static service configuration;
- [ ] repeatable `-H`/`--header` reaches the shared static service configuration in order;
- [ ] non-TLS builds retain clear TLS-flag rejection;
- [ ] no new parser/DNS/TLS dependency is added.

---

# Track F — Focused regression and compatibility coverage

## Objective

Prove only the behavior added or corrected by this plan. Do not recreate the broad conformance suites already closed under Plans 094–101.

Primary existing test locations should include, as appropriate:

```text
crates/eggserve-python/tests/test_http_server_compat.py
crates/eggserve-python/tests/test_https_server_compat.py
crates/eggserve-python/tests/test_simple_http_handler_compat.py
crates/eggserve-python/tests/test_boundary_hardening.py
crates/eggserve-python/tests/test_parity_matrix.py
crates/eggserve-bin/src/args.rs
crates/eggserve-bin/tests/cli_validation.rs
eggserve-core static-service/planner tests nearest the changed configuration path
```

Do not create a new parity framework.

## F1. `default_content_type` tests

Cover at least:

- stock default unknown extension -> `application/octet-stream`;
- subclass/custom configured fallback -> selected type;
- GET and HEAD parity;
- range response retains the same selected type;
- index-file MIME behavior remains correct;
- invalid fallback value fails closed;
- known suffix and `extensions_map` continue to take precedence.

## F2. `extra_response_headers` tests

Cover:

- one custom header on direct-file 200 GET;
- equivalent HEAD metadata;
- ordered duplicate safe headers;
- index-file 200;
- directory-listing 200 when enabled;
- absent on 206;
- absent on 304;
- absent on directory redirect;
- absent on error response;
- attempted `Content-Type`/`Content-Length`/hop-by-hop override cannot replace canonical values;
- CR/LF/NUL value rejected;
- malformed pair structure rejected;
- exact-stock partial remains native fast path.

Include one instrumentation assertion that no Python callback/GIL path is used for the exact-stock configuration with safe static metadata.

## F3. `send_error()` tests

Cover:

- default known error code;
- unknown status code fallback if supported by the current `responses` contract;
- custom `message=`;
- custom `explain=`;
- subclass custom `error_message_format`;
- subclass custom `error_content_type`;
- HTML escaping of untrusted message/explanation;
- HEAD suppresses body;
- 204/205/304 and representative 1xx suppress generated body;
- generated body stays within configured response limit;
- malformed template -> sanitized generic 500;
- sentinel exception/template data does not leak through operational diagnostics.

## F4. Header-view tests

For each implemented read-only helper, compare representative behavior to stdlib where practical:

```text
mixed-case lookup
duplicate fields
missing key
Content-Type with charset
Content-Type with parameters
invalid/odd-but-accepted parameter syntax within the bounded contract
ordered keys/values/items
```

Do not add tests for unsupported mutation APIs merely to increase parity percentages.

## F5. Protocol/helper tests

Cover:

- default HTTP/1.1 handler accepted;
- subclass `protocol_version = "HTTP/1.0"` rejected before activation;
- arbitrary invalid protocol rejected;
- `request_version` remains populated from the request;
- `log_date_time_string()` format is stable;
- subclass logging overrides still receive the existing bounded calls where documented.

## F6. CLI tests

Cover:

- `--content-type` valid and invalid values;
- one and multiple `-H` values;
- long `--header` alias;
- malformed header arity;
- unsafe header rejection;
- new flags compose with positional PORT/DIRECTORY semantics;
- `--bind localhost` if Track E1 lands;
- unresolved hostname failure if Track E1 lands;
- wildcard guard after resolved host where practical without external DNS dependence;
- TLS cert-only combined PEM success under `--features tls`;
- key-without-cert rejection;
- non-TLS binary rejection remains clear.

Hostname tests must use `localhost` or a controlled local resolver assumption; do not depend on public DNS/network access.

## F7. Version strategy

Do not broaden routine CI solely to Python 3.15 beta for this plan.

The installed-wheel tests should assert EggServe's declared contract directly. If the development environment has Python 3.15 available, a small differential smoke against stdlib is useful, but it is supplementary rather than a new CI requirement.

## Track F acceptance criteria

- [ ] every new public behavior has focused regression coverage;
- [ ] native fast-path preservation is tested directly;
- [ ] no network-dependent test is added;
- [ ] no timing-heavy concurrency test is added;
- [ ] no broad stdlib-copy test corpus is introduced;
- [ ] existing static/range/conditional/security suites remain green.

---

# Track G — Reconcile active documentation and capability matrices

## Objective

Update only active normative documentation affected by the reviewed findings. Historical completed plans may remain historical.

Required files to audit:

```text
README.md
docs/python-http-server-compatibility.md
docs/python-api.md
docs/cli.md
docs/library-capability-matrix.md
docs/http-primitives.md, only where a changed static metadata contract is relevant
docs/tls.md, if CLI combined-PEM behavior changes
architecture/eggserve-python.md
architecture/eggserve-bin.md
architecture/eggserve-core.md, only if a public static builder changes
```

## G1. Python compatibility matrix

Add/clarify rows or prose for:

```text
default_content_type
extra_response_headers
read-only HTTPMessage-style header helpers
send_error customization level
fixed HTTP/1.1 protocol_version behavior
log_date_time_string
```

Continue to label raw sockets, `handle_request()`, `translate_path()`, raw `list_directory()`, async handlers, arbitrary SSL contexts, and unbounded streaming as intentional incompatibilities.

Do not claim complete drop-in compatibility with `socketserver`.

## G2. Correct redirect documentation

The capability matrix currently has a generic `Redirects` row that can be read as saying EggServe has no redirects, while the static service implements canonical directory redirects (`/dir` -> `/dir/`) and preserves the query string.

Replace the ambiguous row with precise wording, for example:

```text
Directory canonicalization redirect: implemented
General application redirect abstraction: intentionally unsupported/not applicable
```

Do not add a generic redirect framework.

## G3. Correct TLS documentation

Ensure active docs state:

- Python `HTTPSServer`/`ThreadingHTTPSServer` are supported;
- TLS is rustls-backed and HTTP/1.1 ALPN only;
- Python certfile with `keyfile=None` uses a combined PEM;
- CLI TLS flags are feature-gated for source-built binaries;
- installed Python wheel includes the native TLS-enabled path as currently packaged;
- CLI combined-PEM behavior matches Track E after implementation;
- encrypted-key password support remains unavailable.

Fix the library capability matrix if its Python TLS cells remain stale.

## G4. Correct bind terminology

If Track E1 lands, keep `HOST[:PORT]` wording and document one-time startup resolution.

If Track E1 is rejected under its bounded implementation condition, change all CLI-facing references from `HOST` to `IP`/`IP[:PORT]` so documentation matches reality.

Do not leave the present mismatch in place.

## G5. Python-version framing

Describe the new options as a source-familiar additive surface aligned with Python 3.15, but do not make EggServe's minimum Python runtime version depend on Python 3.15. The wheel remains usable on the currently supported CPython versions because these are EggServe API additions implemented in its own facade.

## Track G acceptance criteria

- [ ] README and compatibility docs remain concise and truthful;
- [ ] implemented directory redirects are no longer hidden by a generic unsupported row;
- [ ] Python TLS support appears correctly in the capability matrix;
- [ ] CLI TLS feature gating and combined-PEM behavior are documented;
- [ ] bind terminology matches implementation;
- [ ] HTTP/1.1-only behavior is explicit;
- [ ] intentional incompatibilities remain explicit and are not relabeled as TODOs;
- [ ] no normative document depends on understanding historical plan numbers.

---

# Track H — Verification, proportional qualification, and closure

## Objective

Verify the pass with existing tooling and record concise evidence without repeating the overbuilt qualification patterns that earlier plans intentionally reduced.

## H1. Required local verification

At minimum run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-bin --features tls
cargo test --doc -p eggserve-core
./scripts/verify.sh full
```

The full verification path should continue to cover installed-wheel compatibility tests and package dry-runs. Do not add another wrapper script.

## H2. Routine hosted CI

Require the existing routine CI jobs to pass on the final implementation/documentation SHA.

Do not add new jobs or matrices for this plan.

## H3. Platform qualification rule

Do **not** automatically require a full manual platform/release qualification merely because Python helper methods or documentation changed.

Use the existing manual Platform Qualification workflow only if implementation changes one of these cross-platform/native boundaries:

```text
listener/address resolution semantics
TLS certificate/key loading
platform-specific native filesystem behavior
wheel/native-extension composition
```

If Track E1 hostname resolution or Track E2 TLS loading changes production native/CLI startup behavior, run the existing relevant platform qualification on the final SHA. Do not create a new workflow or publish a release.

A manual Release workflow run is not required for a pure compatibility-polish change unless the implementation materially changes wheel composition or an existing release gate requires it.

## H4. Closure record

Update this plan from `READY FOR HANDOFF` to `COMPLETE` only after implementation and required verification land.

Record:

```text
implementation commit(s)
final verified SHA
local ./scripts/verify.sh full result
routine CI run/result
manual platform qualification run/result, only if H3 triggers it
known intentionally unsupported parity items
```

Do not paste megabytes of test output into the plan.

## Track H acceptance criteria

- [ ] local format/clippy/workspace/TLS/doc/full verification passes;
- [ ] installed-wheel tests pass;
- [ ] routine hosted CI passes on final head;
- [ ] manual platform qualification is run only when the defined cross-platform trigger is met;
- [ ] no publication occurs as part of this plan;
- [ ] closure evidence is concise and tied to an exact SHA.

---

# Suggested implementation slices

Keep commits small enough to review. A reasonable sequence is:

```text
1. core/static: add bounded content-type and extra-200-header configuration
2. python: add 3.15 SimpleHTTPRequestHandler metadata surface and preserve fast path
3. python: improve send_error and read-only HTTPMessage compatibility helpers
4. python: enforce HTTP/1.1 protocol_version contract and add helper polish
5. cli: add content-type/header options and bounded bind/TLS consistency changes
6. tests: complete focused regression/parity coverage
7. docs: reconcile compatibility/CLI/TLS/capability matrices
8. closure: record verification evidence
```

These are review slices, not a requirement to create exactly eight commits. Do not combine unrelated cleanup into them.

---

# Rejection conditions

Reject an implementation that does any of the following:

- adds HTTP/1.0, HTTP/2, or HTTP/3 merely for stdlib checkbox parity;
- adds CGI or restores removed/deprecated CGI behavior;
- adds ASGI/WSGI, routing, middleware, proxying, WebSockets, or application-server scope;
- exposes raw sockets, `fileno()`, raw TLS contexts, or authoritative translated host paths to Python;
- makes Python own `Content-Length`, `Connection`, `Transfer-Encoding`, or final `Date` framing;
- lets `extra_response_headers` overwrite static validators/security/framing headers;
- applies `extra_response_headers` to 206/304/redirect/error responses contrary to the selected contract;
- routes exact-stock static requests through Python solely to implement metadata options;
- performs Python filesystem resolution/opening for MIME or header customization;
- silently accepts an unsupported `protocol_version`;
- adds a generic header middleware framework;
- changes `RuntimeConfig.bind` to a broad hostname/multi-listener abstraction solely for CLI convenience;
- adds a DNS/parser/TLS dependency for features available through the standard library/current stack;
- adds `--tls-password-file` or encrypted-key password handling in this pass;
- weakens wildcard/public-bind acknowledgement;
- changes safe static defaults;
- adds new CI/release workflows or automatic publication;
- reopens already-closed filesystem/runtime architecture without a failing test demonstrating a direct requirement.

---

# Final acceptance criteria

Plan 136 is complete only when all applicable items are true:

- [x] `SimpleHTTPRequestHandler.default_content_type` exists with `application/octet-stream` default and controls unknown-type fallback;
- [x] `SimpleHTTPRequestHandler(..., extra_response_headers=...)` is supported through the documented bounded contract;
- [x] CLI `--content-type` and repeatable `-H`/`--header` use the same native static configuration;
- [x] safe extra headers apply only to static 200 responses and cannot overwrite canonical/runtime-owned metadata;
- [x] stock/static metadata configuration preserves the native no-Python-per-request fast path;
- [x] `send_error()` honors the useful stdlib customization hooks and correct body-suppression semantics;
- [x] `self.headers` supports the selected common read-only `HTTPMessage`-style accessors while preserving duplicates;
- [x] unsupported `protocol_version` values fail clearly and HTTP/1.1 remains the only EggServe server mode;
- [x] `log_date_time_string()` and any other selected pure compatibility helpers are present and tested;
- [x] response metadata/logging docs clearly distinguish Python hooks from Rust-owned wire fields;
- [x] the CLI bind `HOST` claim is either implemented through one-time bounded resolution or corrected to IP-only everywhere;
- [x] CLI TLS accepts cert-only combined PEM consistently with the Python facade;
- [x] encrypted-key password handling remains explicitly out of scope;
- [x] directory canonicalization redirects are represented accurately in the capability matrix;
- [x] Python TLS capability is represented accurately in active documentation;
- [x] all new behavior has focused tests in existing suites;
- [x] existing range, conditional, path-confinement, static-fast-path, TLS, and callback hardening tests remain green;
- [x] `./scripts/verify.sh full` passes;
- [x] routine hosted CI passes on the final verified SHA;
- [x] any manual platform qualification is proportional and triggered only by the native bind/TLS conditions defined above;
- [x] no dependency, workflow, release automation, or architectural scope expansion is introduced without an explicit necessity documented in the completion record;
- [x] the plan is marked complete with exact implementation/verification evidence;
- [x] after completion, the `http.server` compatibility workstream returns to maintenance rather than spawning another broad parity roadmap.

## Completion evidence

- Implementation commits: `0abc620` and corrective follow-up `dad5fe9`, pushed to `main`.
- Local verification: `./scripts/verify.sh full`, both locked `dist` CLI builds, and `git diff --check` passed. The full gate covered conformance validation, Rust workspace tests, TLS tests, examples, the installed Python wheel suite, and package dry-runs.
- Routine hosted CI: run `32055672101` passed on `dad5fe9` (Rust and Python jobs).
- Native-boundary qualification: run `32055681455` passed on `dad5fe9` (macOS arm64 product wheel and Windows x86_64 filesystem qualification).
- No new dependency, workflow, release automation, or compatibility-parity roadmap was added. The compatibility workstream returns to maintenance.
