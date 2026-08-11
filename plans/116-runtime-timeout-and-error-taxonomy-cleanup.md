# Plan 116 — Runtime Timeout and Error-Taxonomy Cleanup

## Status

**COMPLETE.**

Depends on Plans 113–115 sufficiently landing that removed subsystems and verification paths no longer distort the runtime/API surface.

This is a semantic cleanup pass, not a new runtime-feature phase.

---

## Goal

Make runtime timeout behavior, error ownership, documentation, and public API boundaries simpler and internally consistent without weakening protocol or security behavior.

Two review findings drive this phase:

1. active documentation can imply a dedicated response-write timeout even though the exposed runtime configuration centers on `header_read_timeout`, `connection_total_timeout`, `handler_timeout`, and `body_read_timeout`;
2. the project currently documents seven distinct error families, some of which may become redundant after Plan 113 removes out-of-scope client/legacy surfaces.

The preferred solution is deletion/relabeling/documentation correction. Do not add mechanisms merely to make old prose true.

---

## Non-goals

Do not:

- add HTTP/2 or HTTP/3 timeout concepts;
- implement a generic middleware/time-budget framework;
- add per-route timeout configuration;
- add adaptive timeout algorithms;
- remove body/framing rejection distinctions that affect wire behavior;
- collapse path-security errors into generic I/O errors;
- expose internal error details to untrusted clients;
- add a new timer unless a concrete reproducible defect demonstrates the existing model is insufficient;
- redesign the server lifecycle state machine without a discovered lifecycle bug.

---

# Track A — Establish actual timeout semantics from code and wire behavior

Inspect:

```text
crates/eggserve-core/src/server/config.rs
crates/eggserve-core/src/server/connection.rs
crates/eggserve-core/src/server/handle.rs
crates/eggserve-core/src/limits.rs
crates/eggserve-bin/src/
crates/eggserve-python/src/server/
crates/eggserve-core/tests/request_body_timeout_interaction.rs
crates/eggserve-core/tests/request_body_cancellation.rs
runtime/lifecycle tests
architecture/runtime.md
architecture/configuration.md
docs/python-api.md
docs/cli.md
docs/security-policy.md
README.md
```

Build a concise truth table for each configured timeout:

| Timeout | Starts | Ends | Applies to | Wire consequence |
|---|---|---|---|---|
| header read | connection/header parsing | headers accepted or deadline | header acquisition | connection/error behavior |
| connection total | connection future begins | connection future completes/deadline | entire HTTP/1 connection | graceful close/termination |
| handler | service call begins | service completes/deadline | one handler invocation | service timeout response/close policy |
| body read | body consumption begins | required body read completes/deadline | buffered/streamed body consumption | body timeout response/close policy |
| graceful shutdown | shutdown begins | drain finishes/deadline | server/task drain | clean vs forced result |

Do not infer a distinct response-write timeout from comments. Confirm whether any code applies one.

### Acceptance criteria

- every timeout field has one documented semantic definition;
- implementation and docs use the same name for the same deadline;
- no timeout is described as idle if it is total, or total if it is idle;
- keep-alive lifetime implications of `connection_total_timeout` are explicitly understood.

---

# Track B — Decide whether `connection_total_timeout` is intentional maximum connection age

Current code wraps the full Hyper connection future in a timeout. That means a healthy keep-alive connection can be terminated when the total deadline is reached.

Decide explicitly whether EggServe wants this behavior.

For the project's hardened, bounded-resource posture, the default preferred decision is:

```text
connection_total_timeout is an intentional maximum connection lifetime,
not an idle timeout and not a dedicated write timeout.
```

Retain that behavior if tests show it is stable and no supported compatibility contract requires indefinite keep-alive.

If retained:

- rename prose, comments, and user-facing descriptions to state maximum/total connection lifetime consistently;
- do not add another timer solely because older docs said “response-write timeout”;
- ensure the timeout value remains configurable through existing supported surfaces where it already is.

Only consider replacing it with separate idle/write semantics if a concrete supported-use defect is demonstrated. Such a change would require a new explicitly approved plan because it materially changes runtime policy.

### Acceptance criteria

- the project makes an explicit maximum-connection-age decision;
- comments do not claim a timer exists when it does not;
- tests cover deadline closure sufficiently to prevent accidental removal;
- no new timer is added absent concrete evidence.

---

# Track C — Remove misleading response-write-timeout claims

Search:

```sh
rg -n "write timeout|response-write|response write|write deadline|connection_total_timeout|total connection" \
  crates docs architecture README.md AGENTS.md plans
```

Classify matches as current docs, source comments, historical plan text, or tests.

Correct active source comments/docs only. Historical plans remain historical unless a statement is being treated as current acceptance evidence.

Preferred wording:

```text
header_read_timeout: deadline for request headers
connection_total_timeout: maximum lifetime of one HTTP connection
handler_timeout: deadline for one service invocation
body_read_timeout: total deadline for request-body consumption
```

If response emission is bounded only indirectly by the total connection lifetime, say exactly that.

### Acceptance criteria

- no active documentation advertises an independent response-write timeout unless code implements one;
- the resource-exhaustion story remains accurate;
- CLI/Python parameter docs match runtime semantics.

---

# Track D — Re-inventory error families after surface deletion

After Plan 113, list every remaining error type and its owner.

Likely categories include:

- path parsing/confinement rejections;
- canonical/request validation errors;
- core top-level library errors;
- server lifecycle/startup errors;
- service invocation errors;
- request-body errors;
- response construction errors.

If client code was removed, client-only error types/conversions must already be gone.

For each remaining type, answer:

1. Is it public, crate-private, or Python-exposed?
2. Does it represent a distinct recovery/action boundary?
3. Does it encode security-relevant classification?
4. Does another error type contain the same cases with only renaming?
5. Is conversion between two families purely mechanical with no information boundary?

### Preserve these distinctions

Do not collapse:

- `PathRejection` variants that identify why a path was denied;
- body-consumption state errors such as already-consumed/incomplete/too-large/timeout where Python or service behavior depends on the distinction;
- response-construction validation errors where malformed handler responses must fail closed;
- lifecycle/startup errors that callers can act on differently from per-request service failures.

### Candidate simplifications

Only if source evidence supports them:

- eliminate top-level variants whose only owner was a removed client/legacy subsystem;
- merge crate-private wrappers that provide no additional boundary or classification;
- reduce duplicate string-only transport/config wrappers when one typed error already owns the information;
- remove conversion impls that exist solely for deleted modules;
- make internal helper errors private rather than publicly re-exported if no supported consumer uses them.

Do not pursue a target number of error types. Simplicity is the outcome, not a numeric quota.

### Acceptance criteria

- every remaining error family has a distinct owner/boundary;
- no security-significant denial information is lost;
- no public error type exists solely for deleted code;
- conversions are understandable and non-cyclic.

---

# Track E — Python exception mapping consistency

Inspect Python exception declarations and conversion paths after Rust cleanup.

Requirements:

- Python `http.server` facade raises stable, useful exceptions where its contract already documents them;
- internal Rust taxonomy need not be mirrored one-for-one in Python;
- no exception class remains unreachable from compiled code unless intentionally reserved and documented;
- request-body specific exceptions remain distinguishable when users can act on them;
- malformed handler responses continue to fail closed without exposing untrusted response content in logs/exceptions.

Delete unreachable client/legacy exceptions if their source subsystem was removed.

### Acceptance criteria

- Python exception classes correspond to reachable supported behavior;
- Rust internal simplification does not unnecessarily broaden Python breaking changes;
- no stale class is kept merely because an old plan listed it.

---

# Track F — Focused tests

Add or adjust only tests necessary to lock the clarified behavior.

Minimum timeout tests should prove:

- header deadline works;
- total connection lifetime closes a long-lived connection according to policy;
- handler timeout remains per invocation;
- body timeout remains body-consumption scoped;
- graceful shutdown timeout remains independent.

Do not create a giant Cartesian-product timeout suite.

For errors, prefer compile-time/API tests plus focused behavioral tests at conversion boundaries.

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
bash scripts/test-python-wheel.sh
```

Run TLS tests only if TLS runtime code or shared timeout transport code changed.

---

## Final acceptance criteria

Plan 116 is complete when:

- `connection_total_timeout` has one explicit, truthful semantic definition;
- active docs/comments no longer advertise a nonexistent independent response-write timer;
- no new timer/framework is added without a demonstrated defect;
- error families have been re-inventoried after product-surface deletion;
- redundant/dead error variants/conversions are removed where safe;
- path/body/response/lifecycle distinctions needed for security or public behavior remain;
- Python exception mappings correspond to reachable supported behavior;
- routine Rust and installed-wheel verification pass.

## Rejection conditions

Reject the implementation if it:

- adds a write-timeout mechanism solely to preserve stale wording;
- silently changes keep-alive lifetime policy without tests/documentation;
- converts all failures into generic strings;
- collapses security denials into indistinguishable I/O errors;
- exposes internal error text or untrusted handler data to clients/logs;
- creates a new error abstraction layer to simplify the existing error abstraction layers.
