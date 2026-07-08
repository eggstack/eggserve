# Plan 002: path confinement and filesystem policy

## Goal

Implement eggserve's security-critical path confinement engine and filesystem policy layer. This is the most important milestone in the project. The server must not serve a file merely because a joined path appears to be under the configured root. Instead, request-target parsing, percent decoding, component validation, platform-specific rejection, dotfile policy, symlink policy, and root confinement must be explicit, testable, and fuzzable.

The deliverable is an independently testable path/filesystem module that can classify a request target as an allowed file candidate, allowed directory candidate, or rejected request before static-file serving is implemented fully.

## Scope

In scope:

```text
request target parsing for origin-form paths
single-pass percent decoding
path length enforcement
component validation
parent/current component rejection
absolute path rejection
NUL/control-character rejection where appropriate
backslash/separator ambiguity policy
Unix path policy
Windows path policy
reserved-name and ADS rejection on Windows
root confinement model
symlink policy scaffolding
fixture-based tests
regression corpus
initial fuzz targets
```

Out of scope:

```text
reading and streaming file contents
serving index files
rendering directory listings
Range requests
cache headers
TLS
Python API
```

## Core invariant

Under safe defaults, no remotely supplied request target may resolve to content outside the configured root, and no denied filesystem object class may be served.

This invariant must be stated in docs and encoded into tests. It should hold even for:

```text
percent-encoded traversal
mixed separators
double-encoded traversal attempts
absolute path attempts
Windows drive prefixes
UNC-like paths
alternate data streams
reserved device names
symlink escape attempts
reparse-point escape attempts where detectable
```

## Module design

Add or refine modules in `eggserve-core`:

```text
path/
  mod.rs
  decode.rs
  request_target.rs
  components.rs
  policy.rs
  rejected.rs
  platform.rs
fs/
  mod.rs
  root.rs
  resolved.rs
  unix.rs
  windows.rs
```

Suggested public types:

```rust
pub struct RequestPath<'a> {
    raw: &'a str,
}

pub struct SafeRelativePath {
    components: Vec<SafeComponent>,
}

pub struct SafeComponent(String);

pub enum PathRejection {
    Empty,
    TooLong,
    UnsupportedUriForm,
    MalformedPercentEncoding,
    InvalidUtf8,
    NulByte,
    AbsolutePath,
    ParentComponent,
    CurrentComponent,
    SeparatorAmbiguity,
    DotfileDenied,
    WindowsPrefixDenied,
    WindowsReservedNameDenied,
    WindowsAlternateStreamDenied,
}

pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(PathRejection),
}
```

The exact names can differ, but the design should separate parsing rejection from filesystem resolution. A malformed request target is not the same as an absent file, and a denied path is not the same as an internal I/O error.

## Request-target policy

Only support HTTP origin-form paths initially:

```text
/path
/path?query
/
```

Reject or ignore unsupported forms intentionally:

```text
absolute-form: http://example.com/path
authority-form: example.com:443
asterisk-form: *
```

For a static server, query strings should not affect filesystem resolution. Strip the query at the request-target parsing stage. Preserve enough information for logging only if logs are sanitized.

## Percent decoding policy

Decode percent encodings exactly once. Reject malformed encodings. Do not decode twice to be helpful.

Examples:

```text
/%2e%2e/etc/passwd -> decodes once to /../etc/passwd -> reject parent component
/%252e%252e/etc/passwd -> decodes once to /%2e%2e/etc/passwd -> component contains literal percent text, not traversal after a second decode
/%ZZ -> reject malformed percent encoding
/%00 -> reject NUL
```

Decide whether paths are UTF-8-only. The recommended initial policy is UTF-8-only because Python users and HTTP clients generally expect this, and it reduces cross-platform ambiguity. If byte paths are supported later, that should be a deliberate compatibility feature.

## Component policy

After decoding, split into components and reject:

```text
empty ambiguous components if policy requires normalization
.
..
absolute/root/prefix components
components containing NUL
components containing path separators for the current or alternate platform
Windows drive-like prefixes
Windows reserved device names
Windows alternate data stream syntax using ':'
```

Consecutive slashes may be normalized or rejected. For simple compatibility, normalizing repeated `/` to a single separator is acceptable, but the behavior should be documented and tested.

Backslash policy should be conservative. On Windows, treat `\` as a separator and reject if supplied in a URL path component. On Unix, also consider rejecting backslash by default to prevent cross-platform surprises. If this is too strict for real users, make it an explicit policy later.

## Dotfile policy

Safe default: deny dotfiles and dot-directories.

A path should be denied if any component begins with `.` unless the policy allows dotfiles. This includes:

```text
/.env
/.git/config
/foo/.secret
```

Be careful with `.` and `..`: these should be rejected as structural components independently of dotfile policy.

## Symlink and filesystem policy

Safe default: do not follow symlinks.

The implementation should avoid a naive check-then-open path. On Unix, prefer descriptor-relative traversal over time:

```text
open configured root directory
for each component:
  open child relative to parent directory
  use no-follow behavior when symlink policy denies symlinks
  classify final object
```

It is acceptable for the first implementation to use a simpler model if tests and docs clearly mark it as an interim implementation, but the plan should not stop there. If an interim canonicalize-prefix check is used, it must not be considered final for 1.0.

On Windows, explicitly handle:

```text
drive prefixes such as C:
UNC-like prefixes
reserved names such as CON, PRN, AUX, NUL, COM1, LPT1
alternate data streams such as file.txt:stream
reparse points when symlink policy denies symlinks
case-insensitive path behavior
```

If Windows support cannot be made strong in the first pass, gate platform support honestly rather than pretending Unix assumptions are portable.

## Regression corpus

Add tests for at least:

```text
/../etc/passwd
/%2e%2e/etc/passwd
/%2E%2E/etc/passwd
/%252e%252e/etc/passwd
/foo/../../bar
/foo/%2e%2e/bar
/foo//bar
/foo/./bar
/foo\bar
/%5cetc%5cpasswd
/C:/Windows/System32
/c%3a/Windows/System32
//server/share/file
/.env
/.git/config
/foo/.secret
/CON
/AUX.txt
/COM1
/file.txt:stream
/%00
/%ZZ
```

Add fixture tests for:

```text
normal file inside root
normal directory inside root
missing path
symlink inside root to inside root
symlink inside root to outside root
nested symlink escape
```

Safe defaults should deny symlinks regardless of target. A later opt-in may allow symlinks that remain within root, but that is not required for the first pass.

## Fuzzing

Add initial fuzz targets even if CI does not run them continuously:

```text
fuzz/fuzz_targets/request_target.rs
fuzz/fuzz_targets/percent_decode.rs
fuzz/fuzz_targets/path_components.rs
```

Fuzz assertions:

```text
parser never panics
accepted paths contain no parent/current components
accepted paths contain no NUL
accepted paths are relative
percent decoder never double-decodes
rejections are deterministic
```

Later, add filesystem-backed fuzzing with a generated fixture tree if useful.

## HTTP integration

Once the path module exists, wire it into the placeholder service:

```text
GET /valid -> still returns placeholder until static serving milestone
GET /../x -> 403 or 400 depending on rejection class
GET malformed percent -> 400
GET dotfile with deny policy -> 403
```

Status mapping recommendation:

```text
Malformed syntax -> 400 Bad Request
Denied by policy -> 403 Forbidden
Not found -> 404 Not Found, once filesystem lookup exists
Unsupported URI form -> 400 Bad Request
```

Do not leak local filesystem paths in response bodies.

## Acceptance criteria

This milestone is complete when:

```text
Path parsing and filesystem policy are independent modules with unit tests.
Regression corpus covers traversal, encoding, dotfiles, symlinks, and Windows-specific denial cases.
Safe defaults deny parent traversal, absolute paths, dotfiles, and symlinks.
Malformed percent encodings are rejected.
The service maps path rejections to deterministic HTTP responses.
No real file contents are served before the static serving milestone.
Fuzz targets exist and are documented.
```

## Review checklist

Before merging, verify:

```text
No naive path join is used as the only confinement mechanism.
No double decoding occurs.
No response body leaks absolute local paths.
Windows-specific cases are tested or explicitly gated.
Symlink behavior is default-deny.
Dotfile behavior is default-deny.
Path-policy tests can run without starting the server.
```

## Handoff notes

The next milestone should consume `ResolvedResource` or equivalent and implement actual static-file responses. Do not let static serving bypass this module. Every file-serving code path must pass through the same path confinement layer.
