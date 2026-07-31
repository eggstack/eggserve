# Plan 097 — `SimpleHTTPRequestHandler` Secure Static Integration

## Goal

Implement a Python `SimpleHTTPRequestHandler` that is source-familiar to standard-library users while delegating every security-sensitive filesystem and static-response operation to EggServe's existing Rust core.

This plan completes static-handler compatibility without creating a second static server implementation in Python.

## Required outcome

The following form works from an installed wheel:

```python
from functools import partial
from eggserve.server import ThreadingHTTPServer, SimpleHTTPRequestHandler

Handler = partial(
    SimpleHTTPRequestHandler,
    directory="public",
)

with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

Subclass customization remains possible:

```python
class Handler(SimpleHTTPRequestHandler):
    directory_listing = True
    allow_dotfiles = False
    follow_symlinks = False
    index_pages = ("index.html", "index.htm")
```

The Python handler facade decides which documented handler method is in use. Rust remains responsible for path parsing, root confinement, filesystem resolution, conditional and range planning, response normalization, and file streaming.

## Scope firewall

This plan must not add:

- CGI;
- uploads or filesystem writes;
- arbitrary routing;
- templating;
- virtual hosts;
- multiple roots per handler instance;
- reverse proxying;
- authentication;
- cache configuration framework;
- compression;
- multipart ranges;
- automatic index generation beyond the existing safe listing;
- content transformation;
- custom sendfile or memory-map APIs;
- Python-side file reopening after Rust resolution;
- a replacement MIME database dependency;
- HTTP/2 or HTTP/3;
- TLS classes, which are completed in Plan 098;
- a new test framework or CI workflow.

## Required file inspection

Before editing, inspect at least:

- Plan 096 implementation and final Python server facade
- `crates/eggserve-core/src/service.rs`
- `crates/eggserve-core/src/response.rs`
- `crates/eggserve-core/src/primitives/secure_root.rs`
- `crates/eggserve-core/src/primitives/planner.rs`
- `crates/eggserve-core/src/primitives/body.rs`
- `crates/eggserve-core/src/fs/`
- `crates/eggserve-core/src/path/`
- `crates/eggserve-core/src/mime.rs`
- `crates/eggserve-python/src/server.rs`
- `crates/eggserve-python/src/lib.rs`
- `crates/eggserve-python/python/eggserve/server.py`
- current static responder and body-source Python tests
- static wire tests and filesystem confinement tests
- `docs/security-policy.md`
- `docs/compatibility.md`
- `docs/python-http-server-compatibility.md`

Search for duplicate static logic and existing index handling:

```sh
rg -n "index\.html|index\.htm|handle_directory|directory_listing_response" crates
rg -n "resolve_and_plan|StaticResponder|body_source|FileFull|FileRange" crates/eggserve-python crates/eggserve-core
rg -n "guess_type|mime_for_path|list_directory|translate_path|send_head" crates/eggserve-python docs examples
```

## Architectural rule

`SimpleHTTPRequestHandler` must not independently implement path translation or file serving.

Forbidden design:

```text
self.path
  -> urllib decode in Python
  -> os.path.join(directory, path)
  -> open(path)
  -> wfile.write(file.read())
```

Required design:

```text
self.path
  -> native validated request target
  -> SecureRoot / StaticService
  -> resolver-opened file capability
  -> canonical static response plan
  -> Rust file body stream
  -> Hyper response
```

If the standard handler facade needs customization metadata, add only the smallest internal adapter required to carry the resolved static response through the native callback boundary.

## Track A — Public handler contract

### Constructor

Support the familiar constructor shape:

```python
SimpleHTTPRequestHandler(request, client_address, server, directory=None)
```

Required behavior:

- `directory=None` uses the current working directory captured when the handler/server configuration is constructed, not repeatedly resolved from arbitrary request state;
- path-like values are accepted through `os.fspath()` semantics where practical;
- the root must exist and be a directory before serving begins;
- invalid roots fail before the first request where possible;
- one handler instance serves one request, consistent with the base facade;
- the configured root becomes a native `SecureRoot` or equivalent pinned root.

Do not allow the request target to replace or escape the configured root.

### Class attributes

Provide these documented or EggServe-specific class attributes:

- `index_pages = ("index.html", "index.htm")`
- `directory_listing = False`
- `follow_symlinks = False`
- `allow_dotfiles = False`
- `extensions_map` only if it can be supported as a small MIME override map without bypassing the native MIME fallback.

Security attributes are explicit EggServe extensions. They must be read once into a native static policy for each handler class/server configuration rather than consulted inconsistently during individual filesystem steps.

If subclass attributes are mutated after the server starts, behavior need not change dynamically. Document that policy is captured at startup.

### Methods

Implement:

- `do_GET()`
- `do_HEAD()`
- `send_head()` as a compatibility extension point where practical
- `list_directory(path)` only as a safe compatibility hook; it must not receive an unrestricted host path unless that path is a verified internal capability
- `guess_type(path)`
- `translate_path(path)` only if it can return a non-authoritative display/debug path without encouraging insecure reopening; otherwise explicitly document that it is unsupported rather than returning an unsafe filesystem path.

The implementation should prefer a private `_static_response()` adapter and keep `send_head()` as a thin compatibility method.

## Track B — Static response fast path

### Objective

Allow a Python handler method to return an internal static response token that the native adapter recognizes and converts into the existing Rust file-backed response.

### Required native representation

Add one internal-only response variant sufficient to represent:

- status;
- ordered response headers;
- empty body;
- in-memory bytes;
- resolver-opened full-file body;
- resolver-opened single-range body.

The native adapter already has body-source concepts. Reuse them rather than introducing a parallel file response type.

Correct the current conversion path so `FileFull` and `FileRange` body sources are preserved all the way to Hyper transport. They must not be converted to empty responses or read into Python memory.

### Ownership

- the resolver-opened file handle is one-shot;
- Python may hold an opaque object but must not extract a host path and reopen it;
- returning the response consumes the file body capability;
- duplicate attempts to send the same file response fail clearly;
- the file-stream semaphore permit remains owned for the actual stream lifetime;
- HEAD does not acquire or retain a file-stream permit.

### Callback integration

The base handler adapter from Plan 096 stages ordinary byte responses through `wfile`.

For `SimpleHTTPRequestHandler`:

- `do_GET()` and `do_HEAD()` use the static fast path;
- the handler does not copy a file into `wfile`;
- subclass code that overrides `do_GET()` can still use ordinary `send_response()` and `wfile` behavior;
- a subclass calling `super().do_GET()` receives the static fast path;
- response finalization remains singular and cannot send both a static token and staged `wfile` bytes.

## Track C — Directory canonicalization and index behavior

### Trailing slash redirect

For a resolved directory requested without a trailing slash:

- return 301 Moved Permanently for GET and HEAD;
- construct `Location` from the validated origin-form path;
- append `/` before the query component;
- preserve the query string;
- do not reflect unvalidated control characters;
- return no response body unless a small canonical error/redirect body is already part of project policy;
- include Date and correct Content-Length under Plan 095 rules.

Examples:

```text
/docs        -> /docs/
/docs?x=1    -> /docs/?x=1
/%64ocs      -> redirect based on the normalized safe request-path policy selected by the implementation
```

Do not create open redirects or absolute external URLs.

### Index selection

For a resolved directory with a trailing slash:

1. Try each name in `index_pages` in order.
2. Resolve each candidate through the originating directory capability and static policy.
3. Serve the first regular file found.
4. Do not follow a denied symlink merely because it is named as an index.
5. Do not reopen by joined path.
6. If a candidate is denied, continue or fail according to one documented policy; prefer treating denied candidates as unavailable and continuing to the next safe candidate unless doing so leaks policy details.
7. If no index exists, use listing policy.

The default sequence is exactly:

```python
("index.html", "index.htm")
```

Do not add other names automatically.

### Directory listing

Default remains disabled.

When enabled:

- use the existing policy-filtered native directory enumeration;
- preserve entry limits;
- hide denied symlinks and dotfiles according to policy;
- escape visible text;
- percent-encode path segments;
- preserve CSP, Referrer-Policy, and `nosniff` fields;
- GET and HEAD metadata must match;
- do not expose absolute host paths;
- do not add icons, JavaScript, sorting controls, templates, or stylesheets.

### `list_directory()` compatibility

The standard method normally receives a host path and returns a file-like object. That shape is unsafe and mismatched with EggServe's capability model.

Implement one of these narrow approaches, in order of preference:

1. `list_directory()` accepts an internal resolved-directory object and returns the existing safe HTML bytes or response token.
2. Keep `list_directory()` as an override hook receiving a safe entry sequence and display path, documented as an EggServe-compatible signature.
3. If neither can be source-familiar without exposing an unsafe path, keep the method internal and document the incompatibility.

Do not pass a raw filesystem path that subclass code is expected to reopen.

## Track D — Request and response semantics

### GET and HEAD

- GET returns the representation body.
- HEAD returns the same selected status and representation headers as GET with no body.
- Direct file and directory-index routes use the same planner inputs.
- HEAD range requests use the same range status and headers as GET where the existing supported semantics require it, without streaming bytes.

### Conditional requests

Preserve existing support:

- `If-None-Match`;
- `If-Modified-Since`;
- `If-Range` corrected by Plan 095.

Do not implement write-method preconditions in this plan.

### Byte ranges

Preserve single-range support:

- closed ranges;
- open-ended ranges;
- suffix ranges;
- 206 and 416 metadata;
- full-response fallback for unsupported range units or multiple ranges according to current documented behavior.

Do not add multipart/byteranges.

### Methods

The built-in simple handler provides `do_GET()` and `do_HEAD()` only.

Other methods inherit the base handler's missing-method behavior unless a subclass implements them.

Do not add PUT, DELETE, PATCH, POST, or OPTIONS behavior to the built-in static handler.

### Request bodies

GET and HEAD static fast-path requests must reject or close on request bodies under the existing built-in static policy. Do not pass a GET/HEAD body into filesystem logic.

Custom subclass methods continue to use the bounded base-handler body policy from Plan 096.

## Track E — MIME behavior

Use the native MIME map and safe fallback:

- known extension -> current mapped type;
- text types retain documented charset behavior where current code provides it;
- unknown extension -> `application/octet-stream`;
- `X-Content-Type-Options: nosniff` remains present.

### `guess_type()`

Expose a source-familiar override point without creating two divergent MIME systems.

Preferred design:

- default `guess_type(path)` delegates to a native MIME lookup helper;
- `extensions_map` provides a small Python override map checked before the native fallback;
- only the final safe relative name or suffix is passed to MIME selection;
- no filesystem access occurs in `guess_type()`.

If a subclass override returns an invalid header value, response validation fails closed.

Do not add `mimetypes` database initialization or OS registry probing as a dependency of the hardened default path.

## Track F — Security compatibility divergences

Document and test these deliberate differences from Python `SimpleHTTPRequestHandler`:

1. Directory listing is disabled by default.
2. Dotfiles are denied by default.
3. Symlinks are denied by default.
4. Request paths are parsed once and conservatively.
5. Backslashes and platform-ambiguous components are rejected.
6. Resolver-opened handles are used; subclass code is not given an authoritative translated host path.
7. Unknown MIME types use `application/octet-stream`.
8. Static GET/HEAD bodies are rejected.
9. File streams and connections are bounded.
10. Directory entry counts are bounded.

These are product invariants, not temporary incompatibilities.

## Track G — Tests

Add one focused installed-wheel module, for example:

```text
crates/eggserve-python/tests/test_simple_http_handler_compat.py
```

Reuse existing Rust wire and filesystem tests. Do not duplicate every path rejection case in Python.

### Required Python compatibility tests

#### Basic serving

- direct file GET;
- direct file HEAD;
- unknown MIME fallback;
- MIME override through `extensions_map` or `guess_type()`;
- empty file;
- large file confirms streamed response path rather than Python body buffering through an internal observable or bounded-memory test already present in Rust;
- handler subclass can override GET and bypass static behavior deliberately.

#### Directory behavior

- missing slash redirects;
- query-preserving redirect;
- `index.html` preferred over `index.htm`;
- `index.htm` served when HTML index absent;
- no index plus listing disabled -> documented denial status;
- no index plus listing enabled -> safe listing;
- listing GET/HEAD Content-Length parity;
- listing escapes special characters;
- listing hides dotfiles and denied symlinks.

#### Confinement

At Python level, retain only representative integration cases:

- `..` denied;
- encoded traversal denied;
- symlink escape denied on supported hardened platform test paths;
- dotfile denied;
- valid nested file served.

The full adversarial matrix remains in Rust.

#### Conditional and range

- If-None-Match 304;
- If-Modified-Since 304;
- single range 206;
- unsatisfiable range 416;
- weak If-Range produces full 200;
- date If-Range match produces 206;
- HEAD range has no body and correct headers.

#### File body ownership

- full file body reaches wire;
- range body reaches wire;
- body source cannot be reused;
- file-stream permit releases on completion and disconnect through existing deterministic Rust tests;
- HEAD does not consume a file-stream permit.

### Test restraint

Delete or consolidate old Python `StaticResponder` tests when the same behavior is now covered through the final public handler API and Rust core.

Keep low-level primitive tests only for primitives that remain intentionally public under Plan 098.

Do not increase total test count merely by retaining every old test and adding compatibility duplicates.

## Documentation

Update:

- README quick-start Python example;
- `docs/python-http-server-compatibility.md`;
- `docs/python-api.md`;
- `docs/compatibility.md`;
- `docs/security-policy.md` only where Python handler policy needs clarification;
- `docs/secure-root.md` only if the internal static token contract changes public low-level behavior;
- examples directory with one minimal `SimpleHTTPRequestHandler` example.

Required documentation:

- standard import and construction form;
- `directory=` behavior;
- index order;
- listing opt-in;
- security divergences;
- subclass example;
- conditional/range behavior;
- no raw `translate_path()` guarantee;
- Rust-owned streaming.

Do not add a tutorial framework or multiple redundant examples.

## Suggested commit sequence

1. `fix: preserve native file-backed callback responses`
2. `feat: add secure SimpleHTTPRequestHandler facade`
3. `feat: add directory redirect and index-page compatibility`
4. `feat: add safe listing and MIME override hooks`
5. `test/docs: close secure static handler compatibility`

Keep filesystem policy changes separate from Python facade changes if a core correction is genuinely required.

## Verification

Targeted checks should include:

```sh
cargo test -p eggserve-core --test integration
cargo test -p eggserve-core --test http_wire_correctness
cargo test -p eggserve-core --test streaming_buffer_qualification
bash scripts/test-python-wheel.sh
```

Then:

```sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Do not add a new verification mode or hosted matrix.

## Acceptance criteria

Plan 097 is complete only when all of the following are true on one final commit:

- `SimpleHTTPRequestHandler` imports from `eggserve.server` in an installed wheel.
- The standard `partial(SimpleHTTPRequestHandler, directory=...)` pattern works.
- Direct file GET and HEAD use the Rust static-serving path.
- Full-file and range bodies remain file-backed and streamed by Rust.
- No Python path join/open sequence is used for request-derived paths.
- `index.html` and `index.htm` are checked in order through directory capabilities.
- Directory requests without `/` redirect safely and preserve queries.
- Directory listing remains disabled by default.
- Enabled listings remain escaped, filtered, bounded, and HEAD-correct.
- Dotfiles and symlinks remain denied by default.
- Conditional and single-range semantics remain correct.
- Weak `If-Range` does not produce 206.
- Unknown MIME types remain `application/octet-stream` with `nosniff`.
- Static GET/HEAD request bodies are rejected.
- Representative Python integration tests pass while full confinement coverage remains in Rust.
- Superseded duplicate Python tests are removed or consolidated.
- No CGI, upload, framework, proxy, compression, or multi-range scope was added.
- `verify.sh fast` passes.
- `verify.sh full` passes.
- Both existing routine CI jobs pass on the final commit.

## Completion handoff

The implementation handoff must state:

- final constructor and class attributes;
- exact safe-default divergences;
- how the static fast path crosses the Python/Rust boundary;
- how file handles and body sources remain one-shot;
- directory redirect and index rules;
- which old tests were removed as redundant;
- final verification results and commit SHA.

Do not claim TLS class compatibility under this plan.