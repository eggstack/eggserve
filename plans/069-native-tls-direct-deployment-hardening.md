# Phase 69 — Native TLS Direct-Deployment Hardening

## Goal

Qualify eggserve’s intentionally minimal rustls server mode for bounded direct public HTTPS deployment. Strengthen handshake admission, timeout, startup validation, truncation/error behavior, shutdown, packaging, and evidence without adding certificate automation or general edge-server features.

## Preconditions

- Plan 067 provides separate handshake and HTTP connection budgets plus deterministic shutdown.
- Existing `tls` feature builds and tests pass.
- Reverse-proxy TLS remains the preferred production profile.

## Non-goals

Do not add:

- ACME or certificate issuance;
- certificate renewal daemons;
- hot certificate reload;
- SNI virtual hosting or multiple certificate selection;
- client-certificate authentication;
- OCSP stapling;
- HTTP/2 or HTTP/3;
- TLS passthrough or reverse proxying;
- generic TLS library APIs beyond what the static server requires.

## Supported direct-TLS contract

The intended direct profile is:

- HTTP/1.1 over rustls;
- TLS 1.2 and TLS 1.3 only, subject to current rustls defaults/policy;
- one configured certificate chain and private key;
- certificate/key loaded and validated at startup;
- restart required for rotation;
- bounded concurrent handshakes;
- bounded handshake duration;
- static GET/HEAD service with normal eggserve limits;
- no edge-server certificate or routing features.

## Track A — TLS configuration policy

Audit and document:

- enabled protocol versions;
- cipher suite/provider policy inherited from rustls;
- ALPN behavior, limited to HTTP/1.1 or absent as appropriate;
- certificate-chain parsing;
- supported key formats;
- encrypted-key rejection;
- key/certificate mismatch detection;
- empty/multiple-key behavior;
- trust implications of operator-supplied certificates;
- random provider/crypto initialization errors.

Avoid manually freezing cipher suites unless there is a strong security/compatibility reason. Prefer supported rustls defaults with explicit protocol-version bounds.

## Track B — Startup validation

Fail before binding or accepting traffic when:

- certificate file is missing/unreadable;
- key file is missing/unreadable;
- only one of cert/key is supplied;
- chain is empty;
- PEM is malformed;
- key is absent or ambiguous;
- key format is unsupported;
- key and certificate do not match;
- TLS provider initialization fails;
- unsupported feature/configuration combination is requested.

Errors must be actionable but must not print key material. File paths may appear only according to existing sanitized operator-error policy.

Add tests for oversized but bounded certificate chains and input files to prevent unbounded startup memory use.

## Track C — Handshake admission and timeout

Use the Plan 067 handshake budget.

Required behavior:

- handshake permit acquired before expensive TLS work;
- active handshake cap separately configurable or derived with explicit policy;
- total handshake deadline independent of HTTP header deadline;
- slow/stalled clients release permit at deadline;
- malformed handshakes release permit promptly;
- successful handshake transfers cleanly into HTTP connection lifecycle;
- shutdown cancels pending handshakes;
- no unbounded queue between TCP accept and handshake execution;
- saturation behavior is deterministic and observable.

Test exact permit counts and repeated saturation recovery.

## Track D — Handshake abuse corpus

Test raw and TLS-tool-generated cases:

- connect and send nothing;
- partial ClientHello at every major boundary;
- very slow ClientHello;
- malformed record headers;
- oversized declared record lengths;
- unsupported protocol versions;
- unsupported cipher offerings;
- invalid extensions;
- duplicated/odd SNI values even though no virtual hosting exists;
- ALPN values excluding HTTP/1.1;
- plaintext HTTP sent to TLS port;
- abrupt disconnect;
- repeated failed handshakes;
- many concurrent handshakes at and above budget.

Assertions:

- no panic;
- bounded CPU/memory;
- bounded permit lifetime;
- no HTTP service invocation before successful handshake;
- sanitized logs;
- deterministic close.

## Track E — Established TLS connection behavior

Run the HTTP conformance and lifecycle subsets over TLS:

- GET/HEAD;
- range and conditional responses;
- malformed HTTP framing after TLS success;
- keep-alive limits;
- slow headers;
- slow readers;
- client disconnect during file stream;
- connection maximum requests;
- body rejection/static policy;
- graceful shutdown;
- forced shutdown.

Plaintext and TLS paths must share the same HTTP framing and static policy behavior.

## Track F — TLS truncation and close semantics

Test:

- client closes without TLS close-notify during header read;
- client truncates during request body where generic body-enabled tests apply;
- client truncates during response read;
- server graceful shutdown with close-notify where supported;
- forced shutdown without indefinite wait;
- TCP reset during response;
- peer closes after request before reading response.

Define which errors are logged and at what category/level. Routine hostile disconnects should not flood high-severity logs.

## Track G — Certificate rotation contract

Retain restart-only rotation.

Document safe operator procedure:

1. Write new certificate/key atomically outside active files.
2. Validate permissions and pair.
3. Replace files atomically if desired.
4. Restart/recreate eggserve.
5. Verify certificate from an external client.

A running server may retain its loaded configuration until restart. Do not add watchers or hot reload.

Test restart with changed certificate and ensure old process shutdown is bounded.

## Track H — Key-file posture

Where platform semantics permit, add operator warnings for clearly unsafe key-file access, such as world-readable Unix permissions. Warnings must not make portable builds unreliable.

On Windows, document ACL responsibility rather than attempting a broad ACL policy engine.

Never modify key permissions automatically.

## Track I — Packaging strategy

Decide and document whether direct-TLS artifacts are:

- published standalone binaries with `tls` enabled;
- source-build only;
- or a separate named artifact set.

Published Python wheels currently bundle a non-TLS binary unless policy changes. Do not silently change wheel behavior without explicit package contract, tests, and size review.

For every TLS-capable artifact:

- test installed binary outside source tree;
- record feature set;
- verify `--help` and startup behavior;
- verify certificate/key failure cases;
- run live HTTPS smoke and abuse subsets;
- bind artifact to source SHA.

## Track J — Observability

Add structured categories for:

- TLS configured/enabled;
- handshake admitted/rejected;
- handshake timeout;
- protocol/version failure;
- certificate/config startup failure;
- peer abort;
- established TLS connection;
- graceful/forced TLS shutdown.

Do not log secrets, raw certificate contents, private-key metadata beyond necessary operator errors, or unsanitized peer input.

## Required tests

- TLS feature clippy/test on Linux, macOS, and Windows;
- startup validation matrix;
- handshake saturation and timeout;
- malformed handshake corpus;
- established HTTP corpus subset;
- direct HTTPS large-file/range streaming;
- graceful and forced shutdown;
- certificate replacement plus restart;
- installed TLS-capable artifact;
- proxy profile remains unaffected by TLS feature changes.

Use local generated test certificates with deterministic test tooling. Do not require public network or ACME.

## Release criteria

Add required direct-TLS profile gates for:

- startup validation;
- handshake budget;
- handshake abuse corpus;
- TLS HTTP conformance;
- TLS resource recovery;
- TLS shutdown;
- installed artifact;
- feature/dependency audit.

These gates are required only for direct-TLS production profiles, but TLS changes must not invalidate or weaken reverse-proxy origin profiles silently.

## Acceptance criteria

- Handshakes have independent concurrency and time bounds.
- Malformed or stalled TLS clients cannot retain unbounded resources.
- HTTP behavior after TLS is identical to qualified plaintext origin behavior.
- Startup rejects invalid certificate/key configurations deterministically.
- Shutdown is bounded during both handshake and established connections.
- Certificate rotation remains restart-only and documented.
- No ACME, virtual hosting, multi-cert, client-auth, OCSP, or HTTP/2 scope is added.
- TLS-capable artifacts have installed-package evidence tied to the final SHA.

## Stop conditions

Do not claim direct-TLS production support if:

- handshake saturation or timeout is not independently enforced;
- malformed handshakes cause unbounded CPU/memory or permit leakage;
- installed TLS artifact evidence is absent;
- certificate/key mismatch is discovered only after accepting traffic;
- supporting deployment requires ACME, virtual hosting, or HTTP/2.

## Handoff

Plan 070 includes TLS request-sequence and truncation seeds in stateful live-socket fuzzing. Plan 072 carries TLS-capable artifacts into soak and provenance qualification.
