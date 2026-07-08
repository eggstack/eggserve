# Plan 003: static file serving MVP

## Goal

Implement the first real static-file serving path using the path confinement and filesystem policy layer from Plan 002. The result should safely serve regular files with `GET` and `HEAD`, handle index files conservatively, deny directories unless listing is explicitly enabled, and generate deterministic HTTP responses with conservative headers.

This milestone should produce a usable local static server, but not yet a fully production-hardened release. Resource limits, slow-client handling, Python wheel packaging, TLS, and advanced cache/range behavior belong to later milestones.

## Scope

In scope:

```text
serve regular files under configured root
GET and HEAD support
Content-Length
Content-Type with conservative MIME policy
Last-Modified
basic ETag policy if simple and documented
index file handling
403 for denied directories when listing disabled
optional generated directory listing behind explicit policy
HTML escaping for generated listings
conservative security headers
HTTP status mapping for file/path outcomes
integration tests using temporary fixture directories
```

Out of scope:

```text
Range requests
compression
upload/write methods
custom user handlers
TLS
HTTP/2
CORS
authentication
Python API
advanced cache negotiation beyond basic validators
```

## Required dependency review

Before adding dependencies, check whether each can be avoided or feature-gated.

Possible additions:

```text
mime_guess or tiny internal extension table
httpdate for Last-Modified formatting, if needed
```

Avoid:

```text
libmagic bindings
template engines
compression crates
frameworks
reqwest
```

A small embedded MIME map may be preferable for auditability. If `mime_guess` is used, document why its dependency surface is acceptable. Unknown types should always fall back to `application/octet-stream`.

## Static serving pipeline

Every request must pass through the same pipeline:

```text
HTTP method policy
request target/path policy
filesystem resolution
resource classification
response construction
body streaming
logging/telemetry hook
```

No code path should open a file directly from the raw request target. The path confinement module owns request path interpretation.

## Method behavior

Supported methods:

```text
GET
HEAD
```

Unsupported methods:

```text
405 Method Not Allowed
Allow: GET, HEAD
request body should not be consumed beyond what Hyper requires
```

`HEAD` should return the same status and headers as `GET` would return, except without the response body. Pay attention to directory handling and errors: `HEAD /missing` should return the same status as `GET /missing` without a body.

## File response behavior

For regular files:

```text
200 OK
Content-Length: exact file length
Content-Type: extension-based or application/octet-stream
Last-Modified: file mtime when available
ETag: optional but deterministic if implemented
X-Content-Type-Options: nosniff
body: streamed file bytes for GET, empty for HEAD
```

Do not read entire large files into memory. Use streaming bodies. The simplest initial implementation can read chunks asynchronously through Tokio file APIs. Zero-copy/sendfile support can be investigated later and should not block this milestone.

If file metadata changes between classification and streaming, fail closed or return an internal error without leaking filesystem details. Do not try to be clever in the MVP.

## MIME policy

Default behavior:

```text
known extension -> mapped type
unknown extension -> application/octet-stream
no content sniffing
always add X-Content-Type-Options: nosniff
```

Avoid deriving MIME from file contents in the initial version. Content sniffing creates ambiguity, dependency pressure, and possible security surprises.

## Index behavior

Default index policy:

```text
if path resolves to directory and index.html exists, serve index.html
if no index exists and directory listing is disabled, return 403
if directory listing is enabled, generate a safe listing
```

Consider supporting only `index.html` initially. Additional names can be a later configurable feature.

Index lookup must also pass through filesystem policy. If symlinks are denied, an `index.html` symlink should be denied. If dotfiles are denied, a hidden index path should not be served through a directory trick.

## Directory listing behavior

Directory listing should remain disabled by default. If implemented in this milestone, it must be explicitly enabled by policy or CLI flag.

Generated listings must:

```text
HTML-escape all names and hrefs
avoid JavaScript
avoid inline event handlers
avoid remote assets
include a conservative Content-Security-Policy
not expose absolute filesystem paths
not expose owner/group/device metadata
sort deterministically
avoid terminal/control-character injection in visible names
```

Recommended generated headers:

```text
Content-Type: text/html; charset=utf-8
X-Content-Type-Options: nosniff
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'
Referrer-Policy: no-referrer
```

If inline style is avoidable, omit it and use `style-src 'none'`. Keep the HTML minimal.

If directory listing is not implemented in this milestone, return 403 and document listing as a later opt-in feature. That is acceptable and safer.

## Error response behavior

Map outcomes consistently:

```text
malformed request target -> 400 Bad Request
policy-denied path -> 403 Forbidden
missing file -> 404 Not Found
path resolves to directory without index/listing -> 403 Forbidden
unsupported method -> 405 Method Not Allowed
unexpected I/O error -> 500 Internal Server Error
```

Response bodies should be short and generic. Never expose absolute local paths, raw OS errors, usernames, home directories, or internal policy internals in HTTP bodies.

## Cache validators

Implement `Last-Modified` if metadata is available.

ETag is optional for the MVP. If implemented, document semantics. A metadata-derived ETag is not a cryptographic content identity and may be weak. Use weak ETags if appropriate.

Do not implement full conditional request handling unless there is time to test it. It is acceptable to emit validators without honoring every conditional header in the first MVP, but docs should be honest. Prefer adding conditional handling in a later polish milestone.

## Tests

Use temporary fixture directories and integration-style tests.

Required tests:

```text
GET existing file returns 200 and body
HEAD existing file returns 200 and empty body
GET missing file returns 404
GET denied dotfile returns 403
GET symlink returns 403 under safe default
GET directory with index serves index
GET directory without index returns 403 when listing disabled
GET unsupported method returns 405
Content-Length matches file length
Content-Type defaults to application/octet-stream for unknown extension
Content-Type known extension is mapped conservatively
response does not leak absolute root path on error
```

If directory listings are implemented:

```text
listing disabled by default
listing requires explicit policy
listing escapes `<`, `>`, `&`, quotes, control-like names
listing does not include absolute filesystem path
listing denies hidden entries when dotfile policy denies dotfiles
```

## Performance baseline

Add a simple local benchmark or documented manual command, but do not over-optimize yet. The only performance requirement for this milestone is that large files are streamed rather than loaded fully into memory.

Suggested manual checks:

```bash
python -m http.server 8000 --directory fixture
cargo run -p eggserve-bin -- --directory fixture --port 8001
curl -i http://127.0.0.1:8001/file.txt
curl -I http://127.0.0.1:8001/file.txt
```

## Acceptance criteria

This milestone is complete when:

```text
eggserve can serve real regular files safely from a configured root.
All file-serving paths go through the path confinement layer.
GET and HEAD semantics are correct for normal files.
Directories are handled conservatively.
Dotfiles and symlinks remain denied by default.
Headers are conservative and deterministic.
Large files are streamed rather than buffered completely.
Tests cover normal, denied, missing, directory, and header behavior.
```

## Review checklist

Before merging, verify:

```text
No raw request path is joined directly to root outside the path module.
No absolute filesystem path appears in HTTP error bodies.
No directory listing is enabled by default.
No Range/compression support was added opportunistically.
No dynamic handler abstraction was introduced.
HEAD responses do not accidentally send a body.
Unknown MIME falls back safely.
```

## Handoff notes

After this milestone, eggserve should be useful for safe local static serving. The next milestone should focus on production hardening: resource limits, timeouts, slow-client behavior, file-serving permits, sanitized logs, and graceful shutdown.
