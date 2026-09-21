# Plan 246 — Python interop typing and internal maintainability

## Purpose

Improve the maintainability and static-development experience of the existing
Python API without changing runtime behavior or public import paths.

The current Python architecture is functionally strong:

- the stock `SimpleHTTPRequestHandler` path can bypass Python callback
  dispatch entirely;
- callback and streaming bridges are bounded;
- filesystem authority remains native and confined;
- the low-level sync/async substrate reuses the shared Rust runtime;
- the wheel uses PyO3 abi3 from CPython 3.11.

The remaining issue is primarily API exposure quality and internal
maintainability. The package ships `__init__.pyi`, `server.pyi`,
`subprocess.pyi`, and `_native.pyi`, but `eggserve.lowlevel` has no
dedicated stub file and the distribution does not yet advertise typed-package
status with `py.typed`. In addition, the native primitive binding file remains
large enough that ownership/review is harder than necessary.

This plan preserves all existing Python runtime behavior.

## Compatibility freeze

Preserve all existing imports and object identities, including:

- top-level `eggserve` exports;
- `eggserve.server` six-class compatibility facade;
- compatibility re-exports from `eggserve.server`;
- `eggserve.subprocess`;
- `eggserve.lowlevel`;
- `eggserve._native` names currently used/documented/tested;
- existing exception inheritance;
- constructor keyword/default behavior;
- sync and async request/response/tunnel semantics;
- `SimpleHTTPRequestHandler` fast-path eligibility.

Do not rename `StaticPolicy`, `StaticPolicyWrapper`, or related objects in
this campaign. Clarify namespaces/documentation instead.

## Work

### 1. Build a public Python symbol manifest

Before editing, derive a machine-readable or test-owned manifest from:

- `eggserve.__all__`;
- `eggserve.server.__all__`;
- `eggserve.subprocess.__all__`;
- `eggserve.lowlevel.__all__`;
- the native module registration table.

Record constructor/method/property signatures for the public Python surface.

Use this manifest in regression tests so module splitting cannot silently drop
a registration or change a name.

### 2. Add `lowlevel.pyi`

Create a complete stub for the documented public low-level API.

It must include:

- `RuntimeConfig`;
- sync `Server`;
- request/body/response primitives;
- `ServerBodySource`, `ServerSecureRoot`, `StaticResponder`;
- `AsyncServer`, `AsyncRequest`, `AsyncBody`, `AsyncResponse`;
- async tunnel types;
- all re-exported native primitive/error types from `lowlevel.__all__`.

Use concrete protocols/types where stable and useful. Avoid replacing real
semantics with blanket `Any` merely to silence a checker.

For callback and iterable surfaces, model the accepted callable/iterator shapes
accurately enough that examples type check.

### 3. Tighten existing stubs

Review `server.pyi`, `subprocess.pyi`, and `_native.pyi` against the live
implementation.

Focus on:

- return types currently expressed as `Any` despite stable known behavior;
- optionality/defaults;
- iterator/stream callback shapes;
- address tuple types;
- context-manager return types;
- exception types;
- frozen/config object attributes;
- bytes-vs-text distinctions.

Do not claim a narrower input contract than runtime accepts.

### 4. Add static type-checker qualification

Add an installed-wheel typing smoke suite using at least one mainstream
checker; preferably run both Pyright and mypy if dependency/runtime cost is
reasonable.

The fixture should import and type-check representative code for:

- top-level static serving;
- `HTTPServer` and `ThreadingHTTPServer`;
- `SimpleHTTPRequestHandler` subclassing;
- low-level sync server handler;
- low-level streaming response;
- low-level async server;
- tunnel capability;
- subprocess configuration.

Type-check the installed wheel, not only the source tree, so packaging defects
are visible.

Do not add type checkers as production dependencies.

### 5. Add `py.typed` only after the stubs pass

The package may ship a `py.typed` marker only after:

- the low-level stub exists;
- the public manifest and checker fixtures pass;
- wheel composition includes all stub/marker files;
- release smoke confirms installed-package discovery.

If completeness is not credible, close this subtrack as DEFER rather than
shipping a misleading marker.

### 6. Split private native binding implementation

Refactor `crates/eggserve-python/src/lib.rs` into purpose-owned internal
modules while keeping PyO3 names/registration unchanged.

Candidate boundaries:

- exceptions/error conversion;
- method/version/header canonical wrappers;
- path/static policy wrappers;
- secure-root/resolved-resource wrappers;
- body source;
- validation/generation helpers;
- module registration.

Do not split merely to hit a line-count target. The goal is one obvious owner
per binding concept and smaller review units.

The already-separated `src/server/*` bridge remains the model.

### 7. Optional Python-module internal split

If `lowlevel.py` remains difficult to review after typing work, move private
sync/async implementation helpers into underscore-prefixed modules and
re-export from `eggserve.lowlevel`.

This is optional and must not change:

- `__module__` where tests/users reasonably depend on it, unless preserving
  it explicitly;
- pickleability where currently supported;
- import order/cycles;
- documented traceback/error type behavior.

If preserving these details requires disproportionate complexity, keep
`lowlevel.py` monolithic and close this subtrack NO-GO.

### 8. Namespace documentation

Add a concise table explaining:

- `eggserve.server`: stdlib-shaped compatibility facade;
- `eggserve.lowlevel`: in-process native runtime/primitives;
- `eggserve.subprocess`: CLI/subprocess convenience;
- why similarly named policy classes belong to different layers.

Do not add aliases purely to hide the distinction.

## Tests

Required:

- existing Python public API tests;
- existing server/http-server compatibility tests;
- async bridge tests;
- low-level runtime/body/tunnel tests;
- installed-wheel packaging tests;
- new symbol-manifest parity test;
- new type-check fixtures against the installed wheel;
- wheel composition test for `.pyi` files and, if enabled, `py.typed`.

Also ensure the stock static native fast path remains a true no-callback path.

## Qualification

Run:

```sh
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/test-python-wheel.sh
python3 scripts/check-python-release-metadata.py
python3 scripts/check-wheel-composition.py <wheel-dir>
```

plus the new static typing command(s).

Plan 248 owns final multi-platform release-wheel closure.

## Acceptance criteria

- [ ] all current Python public imports/signatures are manifest-tested;
- [ ] `eggserve.lowlevel` has an accurate stub surface;
- [ ] existing stubs match runtime semantics more closely;
- [ ] representative installed-wheel usage type-checks;
- [ ] `py.typed` is shipped only if completeness criteria pass;
- [ ] native binding implementation is decomposed into clear internal owners
      without PyO3 registration changes;
- [ ] no runtime behavior, callback/stream bounds, GIL isolation, or fast-path
      eligibility changes;
- [ ] no public Python rename/removal.

## Non-goals

Do not add raw sockets, arbitrary `SSLContext`, ASGI/WSGI product behavior,
unbounded async queues, a new Python framework layer, or runtime-only features
to justify type annotations.
