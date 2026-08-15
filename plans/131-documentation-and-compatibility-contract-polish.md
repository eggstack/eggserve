# Plan 131 — Documentation and Compatibility-Contract Polish

## Status

**COMPLETE — 2026-08-15.**

Governing roadmap: Plan 128.

Depends on: Plan 129 support evidence and Plan 130 repository/document ownership decisions.

This is a normative documentation cleanup, not another architecture phase. The objective is to make EggServe easy to understand as a CLI, Python library, and Rust library while preserving precise compatibility/security limits.

---

## Problem statement

The current documentation is technically rich but carries traces of the project's iterative hardening history. Several user-facing statements can also be read as narrower than the actual supported library surface. For example, EggServe correctly says it is not a general web framework, but it also exposes a bounded Python custom-handler path and an embeddable Rust `Service` runtime. The final docs should distinguish **product scope** from **library capability** without implying application-framework scope.

The documentation should answer, quickly and consistently:

```text
What is EggServe?
When should I use it instead of python -m http.server?
How do I run it from the CLI?
How do I use the Python http.server-shaped API?
How do I embed it from Rust?
What HTTP behavior does it support?
What security defaults differ from Python?
What compatibility behaviors are intentionally unavailable?
What is the Windows support posture?
What is explicitly out of scope?
```

---

## Track A — Establish one product statement

Use the Plan 128 product statement as the governing wording, adjusted for concision in each document:

> EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped facade.

Important distinctions:

- CLI: static file serving only;
- Python stock `SimpleHTTPRequestHandler`: hardened static serving, native fast path under documented eligibility;
- Python custom `BaseHTTPRequestHandler`: bounded synchronous custom responses, not an ASGI/WSGI server;
- Rust library: embeddable HTTP/1 runtime and service boundary, intentionally low-level and narrow;
- no claim of full `socketserver` compatibility.

Remove or rewrite wording such as "static files only, that is all" where it contradicts documented library custom-service capability. Preserve the non-goal: EggServe itself is not an application framework/server stack.

### Acceptance criteria

- [x] README opening statement is accurate for CLI + Python + Rust surfaces;
- [x] no doc implies arbitrary application-serving scope;
- [x] no doc falsely says the library can only emit static responses;
- [x] terms `static server`, `http.server-shaped facade`, and `Rust service boundary` are used consistently.

---

## Track B — README hierarchy and quickstarts

Reorder README so a new user encounters surfaces in this order:

1. one-paragraph product statement;
2. why EggServe vs `python -m http.server`;
3. CLI quickstart;
4. Python `http.server` replacement quickstart;
5. Rust library quickstart;
6. security defaults;
7. compatibility/support boundaries;
8. installation and deeper references.

Do not turn README into the complete reference manual. Link to the owning docs for detailed tables.

### Required quickstart snippets

#### CLI

Show:

```sh
eggserve

eggserve --directory public --port 9000
```

and one explicit public-bind example with the required opt-in.

#### Python static

Use the canonical stdlib-shaped pattern:

```python
from functools import partial
from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

Handler = partial(SimpleHTTPRequestHandler, directory="public")
with ThreadingHTTPServer(("127.0.0.1", 8000), Handler) as server:
    server.serve_forever()
```

This should be more prominent than subprocess-management helpers because Python `http.server` replacement is the primary library story.

#### Python custom handler

Keep a short `BaseHTTPRequestHandler` example demonstrating source familiarity, but explicitly note bounded in-memory `rfile`/`wfile` behavior and synchronous handlers.

#### Rust static

Add a compact example using the actual public `eggserve-core` server/static API. Do not expose internal modules or direct Hyper use.

### Acceptance criteria

- [x] all three primary surfaces are visible from README without deep navigation;
- [x] canonical Python static facade is prominent;
- [x] subprocess helpers are presented as optional convenience, not primary API;
- [x] Rust example is public-API-only and compile-checked; Plans 132/133 may add executable demonstrations.

---

## Track C — Add a concise capability/compatibility matrix

Create one authoritative user-facing matrix, preferably in `docs/python-http-server-compatibility.md` with a shorter summary in README.

Suggested rows:

| Capability | `python -m http.server` | EggServe CLI | EggServe Python | EggServe Rust |
|---|---|---|---|---|
| static GET/HEAD | yes | yes | yes | yes |
| secure loopback default | no/varies by invocation | yes | facade semantics documented | configurable |
| directory listing default | yes | no | no | no by policy |
| symlink following default | yes | no | no | no |
| dotfiles default | served | denied | denied | denied |
| ranges/conditional requests | limited/version-dependent | supported contract | supported static path | supported static path |
| custom handler responses | subclass | no CLI | bounded sync | `Service` |
| raw socket access | yes via socketserver internals | no | no | listener/runtime APIs only |
| `translate_path()` | yes | n/a | intentionally unavailable | hardened resolver primitives |
| ASGI/WSGI | no | no | no | no |

Do not overfit the matrix to Python version trivia. The purpose is product boundary clarity.

---

## Track D — Tighten Python compatibility language

Review `docs/python-http-server-compatibility.md`, `docs/python-api.md`, README, and stubs/comments for consistency.

The contract should clearly separate:

### Supported source-familiar behavior

```text
six public compatibility classes
(host, port) tuples
port 0 publication
serve_forever/shutdown/context manager
send_response/send_header/end_headers
rfile/wfile bounded facades
request headers with duplicate-preserving access
SimpleHTTPRequestHandler(directory=...)
GET/HEAD static semantics
extensions_map / bounded guess_type support
TLS facade constraints
```

### Intentional incompatibilities

```text
raw socket ownership
fileno listener access
exact socketserver internals
one-request handle_request mode
raw translated host path
raw list_directory path exposure
thread-per-connection behavior
arbitrary SSLContext / multiple cert selection
async handler coroutines
unbounded streaming Python response body
```

For each intentional incompatibility, give the reason only where useful: Rust transport ownership, security confinement, bounded resource policy, or scope control.

Do not describe these as bugs or TODOs unless the project actually intends to implement them.

---

## Track E — Rust library documentation ownership

Ensure a Rust user can discover:

```text
eggserve-core crate purpose
primitives facade
server module status (experimental before 1.0)
StaticService
RuntimeConfig
Server/ServerHandle lifecycle
Service/service_fn
canonical Request/Response types
TLS feature if relevant
security caveats around extraction APIs
```

Update crate-level rustdoc examples from `ignore` to `no_run` or compilable examples where practical. If an example genuinely cannot compile in rustdoc because it requires async runtime setup, prefer a complete `no_run` example over `ignore` so API drift is caught.

Do not promote internal `fs`, `path`, or `response` modules to public merely to simplify docs.

### Acceptance criteria

- [x] crate root describes the intended public boundary;
- [x] `primitives` and `server` have clearly differentiated stability status;
- [x] at least one static-server rustdoc/example compiles;
- [x] at least one custom-service rustdoc/example compiles;
- [x] no direct Hyper dependency appears in user docs.

---

## Track F — Support and platform truthfulness

Apply Plan 129 evidence.

If Windows reaches full adversarial qualification, update README/SECURITY/support docs to state exactly what was qualified. If qualification is partial, retain a narrowed caveat naming the untested class. If a defect is found, retain the stronger warning.

Keep source-supported platforms distinct from prebuilt wheel targets.

Do not imply macOS x86_64 has a prebuilt wheel if only source support is intended.

---

## Track G — Remove planning-history leakage from normative docs

Normative docs should not require understanding Plans 000–130.

Remove statements whose authority is effectively:

```text
"Plan 086 established..."
"Plan 123 says..."
"after phase N..."
```

Replace with the actual invariant/behavior and link to architecture docs if implementation rationale matters.

Historical plans may retain their original wording and closure records.

`AGENTS.md` may continue to mention plan ranges as repository navigation, but runtime/security behavior must be explained independently of plan numbers.

---

## Track H — Documentation duplication cleanup

After Plan 130 establishes ownership, remove duplicated detailed tables and repeated explanations.

Examples:

- security defaults belong normatively in `docs/security-policy.md`; README summarizes;
- Python compatibility deviations belong in the compatibility doc; README summarizes;
- internal server lifecycle belongs in architecture/runtime docs, not repeated in several user guides;
- release process belongs in release docs, not SECURITY except where vulnerability/yank procedure requires it.

Prefer links over copied paragraphs.

---

## Verification

Documentation examples must be mechanically verified where possible.

Required checks:

```sh
cargo test --doc -p eggserve-core
cargo check -p eggserve-core --examples   # after Plan 132 examples exist
python -m compileall examples             # or narrower relevant Python paths
```

For README snippets that correspond to canonical examples, keep the snippet intentionally aligned with the executable example rather than maintaining two divergent implementations.

Run routine project verification after code/rustdoc changes.

---

## Rejection conditions

Reject documentation changes that:

- claim full drop-in `socketserver` compatibility;
- hide intentional compatibility limits;
- call custom Rust/Python response capability a general web framework;
- make subprocess helpers the primary Python story;
- advertise unsupported Windows hardening;
- expose internal modules to make docs easier;
- duplicate large reference sections into README;
- introduce benchmark marketing claims without stable methodology/evidence;
- retain plan-number references as normative authority.

---

## Final acceptance criteria

Plan 131 is complete when:

- [x] one consistent product statement appears across user-facing docs;
- [x] README presents CLI, Python, and Rust quickstarts;
- [x] canonical Python `http.server` replacement usage is prominent;
- [x] capability/compatibility matrix exists with an authoritative owner;
- [x] intentional Python incompatibilities are explicit and justified where appropriate;
- [x] Rust public API/stability docs are discoverable and accurate;
- [x] Windows/platform claims match the qualification evidence;
- [x] duplicated normative content is reduced;
- [x] planning-history leakage is removed from normative behavior claims;
- [x] executable/rustdoc examples remain mechanically verified.
