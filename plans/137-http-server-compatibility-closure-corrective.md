# Plan 137 — `http.server` Compatibility Closure Corrective

## Status

**READY FOR HANDOFF — 2026-08-18.**

Reviewed implementation baseline:

- Plan 136 handoff: `c63ee647c0bac81ebaef0f2c3372edc7a5d5f91a`
- Plan 136 implementation: `0abc6203789df0d53798c1dcbd8c3cf5a9c590b1`
- Plan 136 corrective implementation: `dad5fe97cb2d5754e6dfd6adbc9908b2c503369c`
- Plan 136 closure record / current reviewed head: `7138055ca8afc3446abebbc46b9ff97a2c30b36b`

Relevant completed work:

- Plan 100 — residual `http.server` correctness
- Plan 101 — `http.server` verification and closure
- Plan 131 — documentation and compatibility-contract polish
- Plan 135 — positional CLI regression and post-audit requalification
- Plan 136 — Python `http.server` compatibility polish

This plan is a **small corrective closure patch** for Plan 136. It is not a new compatibility roadmap and must not broaden the product scope.

---

# 1. Why this plan exists

Plan 136 landed substantially correctly. The implementation added the intended Python 3.15-shaped static metadata surface, improved request-header compatibility, made the fixed HTTP/1.1 contract explicit, aligned CLI hostname/TLS behavior, and retained native ownership of parsing, path confinement, response normalization, and file streaming.

The implementation also received a legitimate corrective follow-up (`dad5fe9`) before closure. That follow-up fixed case-insensitive protection of runtime-owned static headers, preserved duplicate safe extra headers, and repaired HEAD representation-length handling. Routine hosted CI and the existing manual platform qualification both passed on that corrective implementation SHA.

A post-closure review nevertheless found one narrow behavioral mismatch and three small documentation/test-closure drifts:

1. `BaseHTTPRequestHandler.send_error()` can still attach generated error-entity metadata on a `HEAD` response whose status code itself forbids an error entity (1xx, 204, 205, 304).
2. Plan 136 requested focused body-forbidden `send_error()` regression coverage, but the installed-wheel compatibility test currently exercises only ordinary 418 GET/HEAD behavior.
3. The active Python compatibility product matrix incorrectly marks EggServe CLI static metadata flags as unavailable even though `--content-type` and repeatable `-H`/`--header` are implemented.
4. Two small truthfulness drifts remain: the library capability matrix says `Plaintext HTTP/1.x` despite the intentionally fixed HTTP/1.1 server contract, and `_check_native_fast_path()`'s internal docstring still describes the old partial-keyword set even though `extra_response_headers` is now accepted.

None of these findings justify revisiting architecture, adding features, adding dependencies, expanding CI, or reopening the broader compatibility workstream.

---

# 2. Goal

Close the remaining Plan 136 correctness/documentation gap with the smallest defensible patch.

Required outcomes:

1. Make `BaseHTTPRequestHandler.send_error()` match the useful CPython `http.server` entity-generation rule for body-forbidden statuses.
2. Add focused regression tests for GET and HEAD behavior across representative body-forbidden statuses.
3. Correct the active compatibility/capability documentation and the stale internal fast-path docstring.
4. Re-run only the existing verification appropriate to the changed Python compatibility path.
5. Record exact implementation and verification evidence in this plan and return the compatibility track to maintenance.

---

# 3. Non-goals and hard boundaries

This corrective must **not**:

- add HTTP/1.0 server mode;
- add HTTP/2 or HTTP/3;
- add CGI, ASGI, WSGI, routing, middleware, proxying, CONNECT, WebSocket, or application-server behavior;
- expose raw sockets, listener file descriptors, `socketserver` internals, or one-request `handle_request()` mode;
- change filesystem confinement, static path resolution, file opening, or streaming ownership;
- move response framing or transport ownership into Python;
- make Python authoritative for `Connection`, `Transfer-Encoding`, `Date`, or final `Content-Length` normalization;
- add interim `100 Continue` machinery or make `send_response_only()` a raw transport primitive;
- broaden `_HTTPMessage` into a mutable/full `email.message.Message` replacement;
- add encrypted TLS-key password support;
- add a dependency, parser framework, new workflow, release automation, or new platform matrix;
- create another parity roadmap after this patch;
- refactor unrelated Plan 136 implementation merely for style.

The correct patch should be small enough that a reviewer can reason about the entire behavior directly.

---

# 4. Finding A — `send_error()` entity metadata on body-forbidden statuses

## Current behavior

The current Python compatibility implementation conceptually does the following:

```python
body = b""
if code >= 200 and code not in (204, 205, 304):
    body = render_error_entity(...)

...

if body or self.command == "HEAD":
    self.send_header("Content-Type", self.error_content_type)
    self.send_header("Content-Length", str(len(body)))
```

The body-generation rule is mostly correct, but the second condition is too broad.

For a request such as:

```python
class Handler(BaseHTTPRequestHandler):
    def do_HEAD(self):
        self.send_error(204)
```

`body` is intentionally empty because 204 forbids an error entity, yet `self.command == "HEAD"` still causes error-entity headers to be queued.

The native canonical response normalizer correctly owns final body suppression and `Content-Length` semantics, so this does **not** create an illegal payload body. However, the Python producer can still introduce `Content-Type` for an entity that should not exist, and 304 has special representation-length normalization behavior that should not be driven by a synthetic zero-length error entity.

This is a source-compatibility/truthfulness defect, not a transport-safety defect.

## Required semantic model

Use one explicit concept for generated error-entity eligibility:

```text
error_entity_allowed = code >= 200 and code not in {204, 205, 304}
```

This also excludes informational 1xx responses.

Then distinguish **representation generation** from **wire body emission**:

- if `error_entity_allowed` is false:
  - do not render the configured error entity;
  - do not add generated `Content-Type`;
  - do not add generated error-entity `Content-Length`;
  - do not write generated body bytes;
- if `error_entity_allowed` is true:
  - render the bounded error entity exactly once;
  - add `Content-Type` and a representation length for both GET-like and HEAD requests;
  - on HEAD, do not write the body bytes;
  - on non-HEAD, write the body through the bounded `wfile` facade.

The resulting intent is:

```python
if error_entity_allowed:
    body = render_bounded_error_entity(...)
    send Content-Type
    send representation Content-Length
else:
    body = b""

end_headers()

if self.command != "HEAD" and error_entity_allowed:
    wfile.write(body)
```

Do not implement this as a special case in Rust normalization. The bug originates in the Python compatibility producer, so fix it there and retain the canonical normalizer as the final transport backstop.

## Status codes that must be covered

At minimum:

- representative informational response: `100` or another 1xx code accepted by the facade;
- `204 No Content`;
- `205 Reset Content`;
- `304 Not Modified`;
- ordinary entity-bearing error, e.g. `418`, as the control case.

The implementation should operate by rule, not by test-specific branching.

## Unknown/nonstandard status handling

Preserve the existing Plan 136 behavior for status codes within EggServe's accepted 100–599 range that are absent from `responses`:

- default short explanation remains `"???"`;
- default long explanation remains `"???"`;
- entity eligibility is determined by the status class/special body-forbidden codes, not by whether the code exists in `HTTPStatus`.

Do not expand the status-code model in this patch.

---

# 5. Finding B — complete the missing regression coverage

## Primary test file

Use the existing installed-wheel compatibility suite:

```text
crates/eggserve-python/tests/test_http_server_compat.py
```

Do not create a new test harness unless the existing file is genuinely incapable of expressing the assertions.

## Required tests

### B1. Ordinary HEAD error retains equivalent-GET representation metadata

Keep or strengthen the existing 418 HEAD test so it proves:

- status is 418;
- no entity bytes are transmitted for HEAD;
- `Content-Type` is present for the generated error representation;
- `Content-Length` reflects the representation that an equivalent non-HEAD error would send, rather than merely asserting that some content-length field exists.

Prefer comparing GET and HEAD metadata for the same handler/template instead of hard-coding an incidental byte count where practical.

### B2. Informational `send_error()` generates no entity metadata

Exercise one 1xx status through a bounded custom handler and verify:

- no generated error body bytes;
- no generated error `Content-Type`;
- no synthetic error-entity `Content-Length`.

If the HTTP client helper refuses to expose a deliberately unusual final 1xx response cleanly, use the existing raw-socket request helper already present in this test module rather than weakening the assertion. Do not add interim-response semantics.

### B3. 204

For both GET-like and HEAD requests where the handler calls `send_error(204)`:

- no generated body bytes;
- no generated error `Content-Type`;
- no generated error-entity `Content-Length`.

Allow runtime-owned headers such as `Date` to remain outside the compatibility assertion.

### B4. 205

Repeat the same logical assertions for 205.

The canonical runtime treats 205 as body-forbidden; the Python producer should not create an error entity for it either.

### B5. 304

For both GET-like and HEAD requests where the handler calls `send_error(304)`:

- no generated body bytes;
- no generated error `Content-Type`;
- no synthetic zero-length error `Content-Length`.

Do not confuse this test with legitimate 304 representation metadata produced by the static file responder. This test concerns `BaseHTTPRequestHandler.send_error(304)` and its generated error entity only.

### B6. Bounded-rendering behavior remains intact

Retain existing coverage for:

- custom `error_message_format`;
- `error_content_type`;
- escaped `message`/`explain`;
- maximum handler-response size;
- malformed customization failing closed to the established generic 500 path.

If those cases already have adequate coverage elsewhere in the installed-wheel suite, do not duplicate them simply to inflate the patch.

## Test assertion style

Prefer semantic assertions:

```text
status
body bytes
presence/absence of content-type
presence/absence/value of content-length
```

Do not require exact raw header casing or ordering unless that property is itself the contract.

---

# 6. Finding C — compatibility product matrix correction

## File

```text
docs/python-http-server-compatibility.md
```

The current product-comparison row for static metadata hooks lists the current Python `http.server` CLI flags but marks EggServe CLI as `N/A`.

Correct the row so the surfaces truthfully state that EggServe CLI supports:

```text
--content-type
-H/--header
```

Keep the Python column focused on:

```text
SimpleHTTPRequestHandler.default_content_type
SimpleHTTPRequestHandler.extra_response_headers
```

and keep the Rust column aligned with the native static metadata configuration/builder terminology already used by the project.

Do not turn this row into a detailed parser reference; `docs/cli.md` remains the authoritative CLI option reference.

---

# 7. Finding D — HTTP/1.1 capability wording

## File

```text
docs/library-capability-matrix.md
```

The capability row currently says:

```text
Plaintext HTTP/1.x
```

That wording is broader than the product contract established and enforced by Plan 136.

Change it to terminology that cannot reasonably imply an HTTP/1.0 output/server mode, for example:

```text
Plaintext HTTP/1.1
```

or the equivalent existing project terminology.

Do not alter parser/request-version behavior in this documentation-only correction. The purpose is to describe the advertised server/runtime contract truthfully.

Check nearby active documentation for the exact same stale phrase. If the same product claim is duplicated verbatim in directly related active docs, correct it in the same patch. Do not conduct another repository-wide documentation rewrite.

---

# 8. Finding E — native fast-path docstring drift

## File

```text
crates/eggserve-python/python/eggserve/server.py
```

`_check_native_fast_path()` correctly allows a `functools.partial` of exactly `SimpleHTTPRequestHandler` whose keyword names are a subset of:

```python
{"directory", "extra_response_headers"}
```

but its internal explanatory docstring still describes the pre-Plan-136 set containing only `directory`.

Update the docstring to match the actual predicate.

Also state, consistently with the existing implementation/docs, that supported static metadata captured natively does not require Python per-request dispatch.

Do not change the fast-path predicate unless a new failing test proves the code itself is incorrect. This subtrack is documentation-only.

---

# 9. Files expected to change

The patch should normally be limited to:

```text
crates/eggserve-python/python/eggserve/server.py
crates/eggserve-python/tests/test_http_server_compat.py
docs/python-http-server-compatibility.md
docs/library-capability-matrix.md
plans/137-http-server-compatibility-closure-corrective.md   # completion evidence only
```

A directly related existing test file may be touched if necessary, but expansion beyond this list should require an explicit explanation in the completion record.

The following should **not** need implementation changes for this correction:

```text
crates/eggserve-core/src/primitives/canonical.rs
crates/eggserve-core/src/server/static_service.rs
crates/eggserve-core/src/config.rs
crates/eggserve-bin/src/args.rs
TLS implementation
filesystem resolver/confinement code
CI workflows
Cargo dependency manifests
```

If implementation appears to require changing those components, stop and reassess before widening the patch. The reviewed defect is at the Python compatibility producer/documentation layer.

---

# 10. Implementation sequence

## Step 1 — reproduce the residual `send_error()` behavior

Before editing, add or locally stage a focused failing test demonstrating at least one body-forbidden HEAD case, preferably 204 and/or 304.

The failure should prove that the Python handler currently queues generated entity metadata where no generated error entity exists.

Do not use a broad snapshot test.

## Step 2 — make entity eligibility explicit

Refactor only the small `send_error()` block needed to compute one boolean such as:

```python
entity_allowed = code >= 200 and code not in (204, 205, 304)
```

Use that boolean consistently for:

- rendering;
- generated entity headers;
- body writing.

Keep existing status lookup, logging, escaping, template handling, size bounds, and generic fail-closed behavior unchanged unless required by the corrected branch structure.

## Step 3 — verify HEAD representation metadata for ordinary errors

Ensure the correction does not regress the Plan 136 follow-up that preserved representation length on HEAD.

An ordinary 418 GET and HEAD pair should still have equivalent generated representation metadata while only GET transmits the representation bytes.

## Step 4 — add body-forbidden regression matrix

Cover 1xx, 204, 205, and 304 as described in Track B.

Prefer a small helper/table-driven structure inside the existing unittest style if that keeps the test readable. Do not introduce pytest or another test dependency.

## Step 5 — correct the three wording drifts

Update:

- compatibility product matrix CLI static metadata cell;
- library matrix HTTP/1.1 wording;
- native fast-path internal docstring.

No other prose churn.

## Step 6 — run focused verification

Run the directly affected installed-wheel/Python compatibility tests first, then the existing project full gate if the focused suite passes.

## Step 7 — record closure evidence

After the final implementation SHA is pushed and verification is complete:

- mark this plan `COMPLETE`;
- record exact implementation SHA(s);
- record focused test command/result;
- record `./scripts/verify.sh full` result;
- record hosted CI result if the normal push CI runs;
- state explicitly whether platform qualification was or was not rerun and why.

Because this patch does not intentionally alter filesystem or native bind/TLS behavior, **do not trigger manual platform qualification solely for this correction** unless the actual implementation unexpectedly crosses one of those native boundaries.

---

# 11. Detailed acceptance criteria

## `send_error()` correctness

- [ ] error-entity generation is controlled by one explicit body/entity-eligibility rule;
- [ ] informational 1xx `send_error()` responses do not generate error entity bytes;
- [ ] 204 `send_error()` responses do not generate error entity bytes;
- [ ] 205 `send_error()` responses do not generate error entity bytes;
- [ ] 304 `send_error()` responses do not generate error entity bytes;
- [ ] body-forbidden `send_error()` responses do not add generated error `Content-Type`;
- [ ] body-forbidden `send_error()` responses do not add synthetic error-entity `Content-Length`;
- [ ] ordinary entity-bearing errors still use `error_content_type`;
- [ ] ordinary entity-bearing errors still use the bounded configured `error_message_format`;
- [ ] custom `message` and `explain` behavior remains intact;
- [ ] ordinary HEAD errors retain equivalent-GET representation metadata but transmit no entity bytes;
- [ ] non-HEAD ordinary errors still transmit the generated bounded entity;
- [ ] malformed rendering/customization still follows the established generic 500 fail-closed path;
- [ ] Rust remains the final response/framing normalizer.

## Regression coverage

- [ ] existing 418 GET behavior remains tested;
- [ ] existing 418 HEAD behavior is strengthened to validate representation metadata rather than only header presence;
- [ ] one representative 1xx case is tested;
- [ ] 204 is tested;
- [ ] 205 is tested;
- [ ] 304 is tested;
- [ ] GET-like and HEAD semantics are distinguished where relevant;
- [ ] tests do not depend on incidental header casing;
- [ ] no new testing framework/dependency is introduced.

## Documentation truthfulness

- [ ] `docs/python-http-server-compatibility.md` reports EggServe CLI `--content-type` and `-H`/`--header` support correctly;
- [ ] `docs/library-capability-matrix.md` no longer implies a general HTTP/1.x server mode where HTTP/1.1 is the actual contract;
- [ ] `_check_native_fast_path()` documentation names both supported partial keywords (`directory`, `extra_response_headers`);
- [ ] directly related active docs remain mutually consistent after the patch;
- [ ] no unrelated documentation rewrite is included.

## Scope and architecture

- [ ] no core response-normalization change is required;
- [ ] no static-service architecture change is required;
- [ ] no CLI behavior change is required;
- [ ] no filesystem/TLS/bind implementation change is required;
- [ ] no dependency is added;
- [ ] no CI workflow is added or expanded;
- [ ] no release automation is added;
- [ ] no new compatibility roadmap is created.

---

# 12. Verification

Use the repository's existing verification hierarchy. Do not invent a new closure apparatus for this patch.

## Focused Python compatibility verification

Build/install the wheel using the repository's normal development/test mechanism and run the directly affected compatibility suite, including:

```text
crates/eggserve-python/tests/test_http_server_compat.py
```

Also run any existing Python test aggregation command that normally includes the installed-wheel suite if that is the canonical local path.

The completion record must identify the exact command actually used rather than copying a hypothetical command from this plan.

## Rust/workspace safety check

Because no Rust behavior should need to change, broad Rust-specific requalification is not the primary signal. Still run the existing full repository gate:

```sh
./scripts/verify.sh full
```

and require it to pass before closure.

Also run:

```sh
git diff --check
```

before the final commit/push.

## Hosted CI

Allow the existing routine CI to run normally on the implementation SHA. Require success before marking the plan complete if the repository's normal push workflow is available.

Do not add jobs or version matrices for this patch.

## Manual platform qualification

Default decision: **not required**.

Reason: the intended patch touches Python handler semantics, Python tests, and documentation only. It does not intentionally alter native filesystem behavior, hostname binding, TLS loading, wheel architecture, or platform-specific code.

Only rerun manual platform qualification if the actual implementation crosses one of those boundaries unexpectedly. If not rerun, record that explicitly as a proportional verification decision rather than leaving the omission ambiguous.

---

# 13. Reviewer checklist

A reviewer should be able to answer all of the following with `yes`:

- Does a HEAD request for an ordinary generated error still expose the equivalent representation metadata without sending the entity?
- Does a HEAD request for 204/205/304 avoid inventing an error representation merely because the method is HEAD?
- Does a 1xx `send_error()` avoid generated error entity metadata and bytes?
- Is the fix located at the Python compatibility producer rather than compensating in the canonical Rust normalizer?
- Are the new tests specifically capable of catching the original defect?
- Does the static-file path remain untouched?
- Do Plan 136's native static metadata and duplicate-header fixes remain intact?
- Do the active docs now accurately describe CLI static metadata and HTTP/1.1 scope?
- Does the fast-path docstring match the predicate that is actually executed?
- Is the patch small enough to review as a closure correction rather than a new feature phase?

---

# 14. Rejection conditions

Reject the implementation if it does any of the following:

- changes `normalize_response()` or `normalize_metadata()` merely to mask Python `send_error()` producer behavior;
- removes ordinary HEAD representation length for valid entity-bearing error responses;
- emits generated error body bytes for HEAD;
- emits generated error entity metadata for 1xx/204/205/304 solely because the request is HEAD;
- adds special cases only in tests without a single coherent entity-eligibility rule;
- changes static file 304 semantics while fixing handler-generated `send_error(304)`;
- reopens HTTP protocol-version work;
- adds `--protocol`;
- adds CGI/ASGI/WSGI or raw socket compatibility;
- changes the filesystem resolver, TLS, bind path, or CLI parser without a demonstrated necessity;
- adds a dependency;
- adds or expands CI/release machinery;
- creates another broad compatibility/parity phase after completion;
- performs unrelated cleanup that obscures the corrective diff.

---

# 15. Completion record template

When implementation is complete, replace the status at the top of this file with:

```text
COMPLETE — YYYY-MM-DD
```

and append a concise evidence section in this form:

```markdown
## Completion evidence

- Implementation commit(s): `<sha>` — <summary>.
- Focused Python compatibility verification: `<exact command>` — passed.
- Full verification: `./scripts/verify.sh full` — passed.
- Diff hygiene: `git diff --check` — passed.
- Hosted CI: run `<id>` on `<sha>` — passed.
- Manual platform qualification: not rerun because the final diff did not modify native filesystem, bind, TLS, or platform-specific behavior.  # or record the actual run if unexpectedly required
- Scope: no dependency, workflow, release, protocol, filesystem, TLS, or architecture expansion.
```

Do not mark the plan complete before the final implementation SHA has passed the applicable verification.

---

# 16. Final closure criteria

Plan 137 is complete only when all applicable items are true:

- [ ] `send_error()` no longer attaches generated error-entity metadata to body-forbidden statuses merely because the request method is HEAD;
- [ ] ordinary HEAD error responses retain correct equivalent-GET representation metadata;
- [ ] focused tests cover 1xx, 204, 205, 304, and an ordinary entity-bearing error control case;
- [ ] the compatibility product matrix correctly reports EggServe CLI static metadata flags;
- [ ] the library capability matrix describes the fixed HTTP/1.1 server contract truthfully;
- [ ] the native fast-path docstring matches its actual allowed partial keywords;
- [ ] existing Plan 136 static metadata, duplicate-header, hostname-bind, TLS, and protocol constraints remain unchanged;
- [ ] focused installed-wheel compatibility tests pass;
- [ ] `./scripts/verify.sh full` passes;
- [ ] `git diff --check` passes;
- [ ] normal hosted CI passes on the final implementation SHA when available;
- [ ] manual platform qualification is not expanded unless the actual diff crosses the previously qualified native boundaries;
- [ ] no dependency, workflow, release automation, or architectural scope expansion is introduced;
- [ ] this plan is marked complete with exact implementation/verification evidence;
- [ ] the `http.server` compatibility workstream returns to maintenance after this corrective patch.

After these conditions are met, do **not** create another general `http.server` parity plan absent a concrete regression or a deliberate future Python stdlib change that is independently worth adopting.