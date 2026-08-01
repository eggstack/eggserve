# Plan 101 — HTTP Server Verification and Final Closure

## Status

Final verification, documentation, and closure plan for Plans 094–100.

This plan begins only after Plan 100 implementation is complete. It does not authorize new product features. Its purpose is to establish deterministic evidence for the existing runtime behavior, reconcile active documentation, and close the Python `http.server` compatibility workstream on one verified commit.

Baseline planning commits:

```text
f707b06a1a15b0c3c64a8a6eeb882edd4418da4e  Plan 099
d190ff0c6d2f0f8dcd37e7c0c426ec06c5c6db59  Plan 100
```

## Goal

Close the remaining evidence and documentation gaps with a small, high-value verification set covering:

1. shared file-stream admission for custom-service and Python-originated file responses;
2. compatibility address forms and structured peer metadata;
3. strict response-validation and sanitized diagnostics;
4. MIME behavior across GET, HEAD, range, and invalid values;
5. deterministic TLS behavior for custom and static handlers;
6. active documentation and manual-release consistency;
7. same-commit local and hosted verification.

After this plan is complete, Plans 094–101 should be considered closed unless a newly discovered defect directly violates one of the final acceptance criteria below.

## Governing constraints

- Do not add a new roadmap, milestone, verification framework, evidence registry, or CI workflow.
- Do not add a broader platform matrix.
- Do not reintroduce release publication through GitHub Actions.
- Do not add timing-sensitive kernel-buffer tests.
- Do not duplicate coverage already proven at a lower layer unless the Python boundary itself is the subject.
- Do not add dependencies solely for tests.
- Prefer extending existing Rust and installed-wheel test files.
- Keep routine CI limited to the existing `rust` and `python` jobs.
- Keep release cadence manual.
- Do not expand into ASGI, WSGI, routing, proxying, uploads, WebSockets, HTTP/2, or HTTP/3.
- Do not mark plans complete before hosted checks are visible on the final implementation commit.

## Why a separate closure plan is required

The Plan 099 implementation corrected the main code paths, but its closure record claimed broader evidence than the commit contained. In particular:

- the shared file-stream permit path was implemented but not proven with a focused custom-service test set;
- address coverage did not include empty host, explicit wildcard, invalid host, IPv6, and TLS tuple parity;
- malformed body conversion and log sanitization lacked direct tests;
- MIME tests covered only direct GET success cases;
- TLS tests covered custom handlers but not static GET, HEAD, range, or plaintext rejection;
- active documentation retained stale Python-client and historical contract language;
- hosted `rust` and `python` results were not recorded on the same final commit;
- implementation, workflow, tests, and documentation were committed as one opaque pass.

Plan 100 corrects the remaining implementation defects. Plan 101 establishes concise proof and final repository consistency.

## Execution order

```text
A. deterministic file-stream admission tests
B. focused compatibility and response-boundary tests
C. deterministic TLS static-handler tests
D. active documentation cleanup
E. local full verification
F. same-commit hosted verification
G. final closure record
```

Do not update active plan status to `000–101 complete` before Tracks E and F pass.

---

## Track A — Prove shared file-stream admission deterministically

### Objective

Demonstrate that every canonical file-backed response sent through the custom-service runtime path is governed by the same `ServeState::file_stream_semaphore`, and that the owned permit lives for the actual transport body lifetime.

### Test design principles

Use deterministic in-process tests at the canonical-to-Hyper or service boundary. Do not rely on network send-buffer saturation, large wall-clock sleeps, or race-prone assumptions about when a client has consumed bytes.

The preferred seam is the existing canonical response conversion function that accepts the file-stream semaphore.

### Required Rust tests

Add or extend tests to cover the following minimum set.

#### 1. Full-file permit ownership

With a semaphore capacity of one:

1. create a canonical `ResponseBody::File` for a controlled temporary file;
2. convert it to a transport response;
3. do not consume or drop the body;
4. assert that a second file-body conversion receives `FileStreamLimit`;
5. drop or fully consume the first body;
6. assert that another file-body conversion succeeds.

This proves acquisition, lifetime ownership, and normal release without socket timing.

#### 2. Range-file permit ownership

Repeat the ownership test with `BodySource::FileRange`.

The test must confirm that range bodies use the same permit path and release on body completion or drop.

#### 3. Non-file bypass

With the file semaphore exhausted, verify that these conversions still succeed:

- `ResponseBody::Bytes`;
- `ResponseBody::Empty`;
- `ResponseBody::EmptyWithLength` where valid;
- a normalized HEAD response with no file body.

#### 4. Drop/cancellation release

Use direct body drop or an existing cancellation seam to prove that abandoning a file transport body releases the owned permit.

One deterministic drop test is sufficient. Do not add separate timing-heavy disconnect and shutdown socket tests when the permit is structurally owned by the dropped stream state.

#### 5. Custom-service integration result

Add one small runtime integration test proving the documented admission result:

- one held custom-service file body consumes the only permit;
- a second custom-service file response maps saturation to 503;
- after releasing the held response, the service can return the file successfully.

Use a controlled barrier or direct body handle. Do not depend on kernel buffering.

### Python-static proof

Do not create a 32-way Python socket saturation test for the six-class facade.

Instead, prove the architecture through these two facts:

1. `SimpleHTTPRequestHandler` produces the canonical native file body used by the custom-service path;
2. the custom-service transport conversion is covered by the deterministic semaphore tests above.

Retain one installed-wheel static GET/range smoke test to prevent accidental conversion back to buffered Python bytes.

If an existing internal constructor can set `max_file_streams=1` without adding a new public API, one small Python smoke test is acceptable. Do not add a new public compatibility-facade keyword solely for testing.

### Required code assertions

Where practical, add a narrow code-level assertion or test that:

- file permits are acquired only at transport conversion;
- the permit is moved into stream state;
- no permit is acquired for bytes or empty bodies;
- `ResponseBody::File` does not bypass the semaphore in another service path.

### Acceptance criteria for Track A

- Full and range file bodies hold one permit for their transport lifetime.
- Dropping or consuming the body releases the permit.
- Saturation maps to the documented bounded result.
- Byte, empty, and HEAD responses bypass the file semaphore.
- Custom Rust services and Python static responses use the same canonical file-body transport path.
- Tests are deterministic and do not rely on network timing.

---

## Track B — Complete focused compatibility and response-boundary coverage

### Objective

Add only the missing high-value installed-wheel and low-level boundary tests required to prove Plan 100 behavior.

### Address tests

Extend `test_http_server_compat.py` or the nearest existing focused file with:

- `HTTPServer(("", 0), Handler)` succeeds;
- empty host publishes an actual nonzero port;
- explicit `0.0.0.0` succeeds;
- `localhost` succeeds;
- IPv4 loopback succeeds;
- IPv6 loopback succeeds where supported;
- explicit `::` succeeds where supported;
- invalid hostname raises a controlled exception;
- `client_address` is `(str, int)`;
- `server_address` is `(str, int)`;
- IPv6 tuple hosts contain no brackets;
- `bind_and_activate=False` remains unbound until activation;
- `server_close()` prevents later activation;
- the existing CLI public-bind guard still rejects wildcard use without the CLI opt-in.

Use a small helper for IPv6 capability detection. Skip only when socket creation or bind proves the platform lacks the requested capability.

### TLS address parity

Add one assertion in the HTTPS compatibility suite proving:

- `server.server_address` is a tuple;
- the handler sees `client_address` as a tuple;
- the tuple shape matches plaintext behavior.

Do not duplicate all plaintext address cases over TLS.

### Strict response-body conversion tests

Extend `test_boundary_hardening.py` or the nearest low-level callback suite with focused structural response doubles for:

- unknown body kind -> 500;
- `read_all()` raises -> 500;
- `read_all()` returns non-bytes -> 500;
- missing required body -> 500;
- unsupported body object -> 500;
- already-consumed native body -> 500;
- mismatched `Content-Length` -> 500;
- valid header followed by invalid header -> 500 with no partial valid header on the wire;
- explicit empty response remains valid;
- duplicate valid headers remain preserved.

Keep response doubles local to the test file. Do not create a reusable mock-response package.

### Sanitized-log tests

Using the existing logger test seam or stderr capture:

- raise a Python exception containing a unique sentinel secret;
- verify the client receives generic 500;
- verify the sentinel does not appear in operational output;
- return an invalid header or body object containing another sentinel;
- verify that sentinel also does not appear;
- verify a fixed service-error category remains present if the logger exposes event text.

One test per leak class is sufficient. Do not build a broad logging corpus.

### MIME tests

Extend `test_simple_http_handler_compat.py` with:

- `extensions_map` GET;
- `extensions_map` HEAD;
- `extensions_map` range;
- subclass `guess_type()` GET;
- subclass `guess_type()` HEAD;
- subclass `guess_type()` range;
- `super().guess_type()` default behavior;
- unknown suffix remains `application/octet-stream` plus `nosniff`;
- invalid `guess_type()` result -> generic 500;
- invalid `extensions_map` value -> generic 500;
- directory-index behavior matches the documented bounded contract.

Do not test every built-in MIME type.

### No-Python-file-access assertion

Use one narrow assertion that MIME customization does not invoke Python file opening or path translation. Acceptable approaches:

- monkeypatch `builtins.open` only around the request after test setup;
- override `translate_path()` to raise and prove serving still succeeds;
- use an existing handler seam that records forbidden Python filesystem calls.

Do not monkeypatch global filesystem functions across the entire installed-wheel suite.

### Acceptance criteria for Track B

- All supported constructor forms have direct coverage.
- Address tuple behavior is proven for plaintext and TLS.
- Malformed structural bodies fail closed.
- No partial invalid response reaches the wire.
- Exception and invalid-value sentinels do not appear in logs.
- MIME behavior is consistent across GET, HEAD, and range.
- MIME selection performs no Python-authoritative filesystem operation.

---

## Track C — Complete deterministic TLS static-handler coverage

### Objective

Prove that repository-owned TLS fixtures exercise both custom handlers and the native static-response path without plaintext fallback.

### Required fixture policy

Continue using:

```text
crates/eggserve-python/tests/fixtures/localhost-test.crt
crates/eggserve-python/tests/fixtures/localhost-test.key
```

Requirements:

- clearly test-only;
- no production use guidance;
- no runtime certificate generation;
- no dependency on the system `openssl` executable;
- no new Python cryptography dependency;
- excluded from wheel artifacts if tests are not packaged, consistent with current packaging policy.

### Required HTTPS tests

Add focused cases for:

1. custom handler GET and `request.scheme == "https"`;
2. `SimpleHTTPRequestHandler` static GET;
3. static HEAD with GET-equivalent `Content-Length` and no body;
4. static range response over TLS;
5. TLS `client_address` and `server_address` tuple shape;
6. missing certificate fails before readiness;
7. missing key fails before readiness;
8. unsupported ALPN is rejected;
9. unsupported password is rejected;
10. server shutdown/context management completes;
11. a plaintext request to the TLS listener does not receive a valid HTTP plaintext response.

Do not duplicate Rust PEM parser cases or certificate-chain validation cases that are outside the facade contract.

### Plaintext fallback test

Use a bounded raw socket attempt against the TLS listener and assert that it does not return a valid plaintext `HTTP/1.1 200` response.

Avoid asserting an exact TLS alert byte sequence, which can vary across rustls versions.

### Acceptance criteria for Track C

- Installed-wheel verification always performs successful HTTPS traffic.
- Static GET, HEAD, and range work over TLS.
- No test can silently skip due to missing `openssl`.
- Plaintext does not succeed on the TLS listener.
- No new runtime or test dependency is introduced.

---

## Track D — Reconcile active documentation

### Objective

Make active project guidance describe the final Plans 094–101 implementation accurately and remove stale Python-client, compatibility, and closure claims.

### Required files to audit

At minimum review:

- `README.md`
- `AGENTS.md`
- `.opencode/skills/eggserve-dev/SKILL.md`
- `architecture/overview.md`
- `architecture/eggserve-core.md`
- `architecture/eggserve-python.md`
- `architecture/runtime.md`
- `architecture/testing-and-conformance.md`
- `docs/api-stability.md`
- `docs/compatibility.md`
- `docs/http-primitives.md`
- `docs/http-response-planning.md`
- `docs/library-capability-matrix.md`
- `docs/non-goals.md`
- `docs/python-api.md`
- `docs/python-http-server-compatibility.md`
- `docs/release-contract.md`
- `docs/release-process.md`
- `docs/security-policy.md`
- `docs/tls.md`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- module docstrings under `crates/eggserve-python/python/eggserve/`
- closure records in Plans 099–101.

### Required corrections

#### Python surface

- The supported Python API is the six-class `eggserve.server` facade.
- `eggserve.lowlevel` is advanced/internal-facing and not a second primary API.
- `eggserve.subprocess` contains optional CLI lifecycle helpers.
- The Python HTTP client is not shipped.
- Remove stale `ClientMethod` guidance from active documentation where it implies a current Python client surface.
- Remove stale callback-server-primary wording.
- Keep historical implementation details only where they remain useful and accurate.

#### Response contract

- Canonical status type accepts `100..599`.
- Standalone handler-produced informational responses follow the documented final-response policy and do not imply interim-response support.
- 205 is body-forbidden.
- Invalid response status, headers, body representation, framing, and length fail closed.
- Duplicate valid headers are preserved.
- No active documentation says status 600–999 is valid.
- No active documentation says invalid fields are silently discarded.

#### File-stream contract

- Every canonical file-backed custom-service response uses the shared runtime file-stream semaphore.
- The permit is owned by the transport body for its lifetime.
- Saturation maps to the documented result.
- Byte, empty, and HEAD responses do not consume file permits.
- Avoid claiming separate Python semaphore logic.

#### Address contract

- Empty host normalizes to the documented wildcard form for the Python compatibility facade.
- Explicit wildcard Python constructor intent is accepted.
- CLI wildcard binds still require `--public`.
- Port `0` publication timing is accurate.
- `client_address` and `server_address` are structured tuples.
- Deliberate `socketserver` lifecycle divergences remain explicit.

#### MIME contract

- `extensions_map` behavior is accurate for direct and index files.
- `guess_type()` scope is stated exactly as implemented.
- GET, HEAD, and range preserve selected MIME metadata.
- Invalid MIME values fail closed.
- Python does not authoritatively translate, open, or probe paths.

#### TLS contract

- TLS uses the shared rustls PEM loader.
- Only HTTP/1.1 ALPN is supported.
- Test fixtures are identified as test-only.
- No documentation presents the test certificate as a deployment pattern.

#### Release policy

- GitHub Actions never publishes to crates.io or PyPI.
- `.github/workflows/release.yml` is artifact-build-only.
- It has no OIDC publication permission, publication environment, publish job, or publish toggle.
- crates.io and PyPI publication remain manual maintainer actions.
- Release cadence remains manual.

### Plan status discipline

Before hosted verification passes, active documentation should say Plans 100–101 are pending or in progress.

Only after the final commit passes local and hosted verification may active plan status be changed to:

```text
Plans 000–101 are implementation-complete.
```

Do not claim completion based on code inspection alone.

### Documentation search pass

Run focused searches such as:

```sh
rg -n "000–0(9[0-9]|100)|000-0(9[0-9]|100)|Plans 000" README.md AGENTS.md architecture docs .opencode
rg -n "600|999|200–999|200-999|status.*999" README.md AGENTS.md architecture docs crates/eggserve-python/python
rg -n "ClientMethod|Python client|native callback|callback server" README.md AGENTS.md architecture docs .opencode
rg -n "publish|PyPI|crates.io|id-token|dry_run" .github docs README.md AGENTS.md
rg -n "guess_type|extensions_map|client_address|server_address|max_file_streams" README.md AGENTS.md architecture docs
```

Review matches manually; do not perform blind global replacement.

### Acceptance criteria for Track D

- Active docs describe the final implementation accurately.
- Stale Python-client guidance is removed.
- Plan ranges are consistent.
- Response, address, MIME, TLS, file-stream, and release contracts match code.
- Historical plans remain historical records rather than being rewritten wholesale.
- No active document claims closure before verification is complete.

---

## Track E — Run final local verification

### Objective

Establish one clean final local result after implementation, tests, and documentation are complete.

### Required commands

Run from a clean checkout of the intended final commit:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features client-tls
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
./scripts/verify.sh fast
./scripts/verify.sh full
```

Do not add another verification mode.

### Clean-checkout requirement

Verification must not depend on:

- untracked fixture files;
- locally generated certificates;
- unstaged code;
- cached wheel contents from another commit;
- a globally installed `eggserve` package;
- a system `openssl` binary for test generation.

Use the existing clean-wheel harness and its isolated installation behavior.

### Record exact results

The handoff must record:

- final commit SHA;
- command names;
- pass/fail result;
- relevant test count reported by the installed-wheel suite;
- any legitimate platform skip, limited to actual IPv6 capability;
- confirmation that TLS tests did not skip;
- confirmation that no new dependency or workflow was added.

Do not copy stale test counts from Plan 099.

### Acceptance criteria for Track E

- Every required local command passes on the same commit.
- The wheel suite includes the new address, response, MIME, and TLS tests.
- No TLS test skips due to missing external tools.
- No undocumented local setup is required.

---

## Track F — Confirm same-commit hosted verification

### Objective

Verify that the existing two routine GitHub Actions jobs pass on the exact final commit.

### Existing CI contract

Do not alter the routine job shape unless a direct defect in the existing workflow prevents execution.

Expected jobs:

- `rust`
- `python`

Expected triggers remain:

- pushes to `main`;
- pull requests targeting `main`.

### Required procedure

After pushing the final implementation commit:

1. identify the exact final SHA;
2. inspect GitHub Actions runs for that SHA;
3. wait only through the normal synchronous review process—do not claim completion before results exist;
4. verify both `rust` and `python` complete successfully;
5. record the run URL or run ID and job conclusions in the closure record;
6. if a job fails, fix the defect in a new reviewable commit and repeat verification on the new final SHA;
7. do not reuse a successful run from an earlier commit.

If the GitHub connector does not expose push-triggered runs reliably, use the repository's normal GitHub UI or `gh run list --commit <sha>` during implementation. The absence of connector data is not evidence of success.

### Workflow-change policy

Do not modify CI simply to obtain a green result unless the workflow itself is incorrect.

Permitted workflow correction:

- a small pin, command, or path correction directly required for the existing jobs to execute the intended checks.

Not permitted:

- removing failing tests to make CI green;
- adding a new matrix;
- adding retries around deterministic failures;
- adding release publication;
- splitting into additional routine workflows;
- introducing an evidence upload system.

### Acceptance criteria for Track F

- `rust` is green on the final SHA.
- `python` is green on the same final SHA.
- No required check is pending, skipped unexpectedly, or associated only with an older commit.
- Routine CI remains two jobs.
- Release publication remains absent.

---

## Track G — Write the final closure record

### Objective

Replace the incomplete or overstated Plan 099 closure evidence with one accurate final record for Plans 094–101.

### Closure-record location

Append a concise final closure record to Plan 101. Update Plan 099 only where necessary to point to Plan 101 as the authoritative final verification record; do not rewrite Plan 099 history.

### Required closure content

Record:

- final commit SHA;
- implementation commits by track;
- exact empty-host normalization rule;
- supported hostname, IPv4, IPv6, and wildcard forms;
- structured `client_address` and `server_address` representation;
- canonical file-stream permit acquisition and ownership path;
- deterministic file-stream tests added;
- saturation result;
- supported Python response representations;
- removed silent body fallbacks;
- sanitized log categories;
- MIME behavior for GET, HEAD, range, and directory indexes;
- TLS fixture location;
- successful custom and static HTTPS cases;
- release workflow disposition;
- focused tests added or consolidated;
- local verification commands and results;
- installed-wheel test count from the final run;
- hosted `rust` run result and URL/ID;
- hosted `python` run result and URL/ID;
- confirmation that no new dependency, workflow, release automation, or scope expansion was introduced.

### Plan status updates

Only after all acceptance criteria pass:

- update active documentation to Plans `000–101` complete;
- mark Plan 100 implementation-complete;
- mark Plan 101 complete;
- state that Plans 094–101 are closed;
- preserve Plan 091 as the controlling CI and manual-release policy.

## Suggested commit sequence

Use a reviewable sequence. A suitable pattern is:

1. `test: prove canonical file-stream admission ownership`
2. `test: close compatibility address and response boundaries`
3. `test: close MIME and TLS facade coverage`
4. `docs: reconcile final HTTP server compatibility contract`
5. `docs: record verified closure for plans 094 through 101`

Plan 100 implementation commits should remain separate and precede these closure commits.

Do not place every implementation, test, workflow, and documentation change into one commit.

## Final acceptance criteria

Plans 094–101 are closed only when all of the following are true on one final commit.

### Runtime correctness

- Empty-host Python construction works deterministically.
- Explicit wildcard Python construction is accepted.
- CLI wildcard safeguards remain unchanged.
- Structured peer and server addresses are correct.
- Malformed response body objects fail closed.
- Handler exception and invalid response values do not leak into logs.
- MIME behavior matches active documentation.

### File-stream hardening

- Every canonical file-backed custom-service response is governed by the shared semaphore.
- Full and range bodies hold permits for transport lifetime.
- Drop and completion release permits.
- Saturation maps to the documented bounded response.
- Byte, empty, and HEAD responses bypass the semaphore.
- Python static responses use the same canonical transport path.

### Compatibility tests

- Empty host, hostname, IPv4, supported IPv6, wildcard, invalid address, and tuple behavior are covered.
- Malformed body and partial-header cases are covered.
- MIME GET, HEAD, range, invalid value, and index behavior are covered.
- No Python-authoritative file operation is introduced.

### TLS tests

- Repository fixtures are used.
- Custom HTTPS succeeds.
- Static HTTPS GET, HEAD, and range succeed.
- TLS address tuples are correct.
- Unsupported configuration fails before readiness.
- Plaintext does not succeed on the TLS listener.
- No test skips due to a missing certificate generator.

### Documentation and release policy

- Active docs consistently describe Plans 000–101.
- Stale Python-client guidance is removed.
- Status, framing, file-stream, MIME, address, and TLS contracts match code.
- GitHub Actions cannot publish to crates.io or PyPI.
- Release cadence and publication remain manual.

### Verification

- All required local commands pass on the final SHA.
- `./scripts/verify.sh fast` passes.
- `./scripts/verify.sh full` passes.
- `rust` CI passes on the final SHA.
- `python` CI passes on the same final SHA.
- The closure record contains actual results rather than unverified claims.
- No new routine workflow, dependency, evidence system, or scope-expanding feature was introduced.

## Stop conditions

Stop and write a narrowly scoped follow-up plan only if closure requires one of the following:

- a second HTTP parser or accept loop;
- raw Python socket ownership;
- a second filesystem resolution or reopen path;
- a new TLS stack;
- a general middleware or response framework;
- a new CI matrix or evidence system;
- release automation;
- expansion into ASGI/WSGI, routing, proxying, uploads, HTTP/2, or HTTP/3;
- weakening an existing security or framing invariant.

Ordinary test or documentation work is not a reason to broaden scope.

## Handoff requirements

The final implementation handoff must include all items from Track G and explicitly answer:

```text
Did Plan 100 land correctly?
Did every Plan 101 local check pass?
Did rust CI pass on the final SHA?
Did python CI pass on the same final SHA?
Can any GitHub workflow publish?
Are Plans 094–101 now closed?
```

Do not answer the last question affirmatively until every final acceptance criterion is satisfied.