# Production-Grade Internet Deployment and Windows Hardening Roadmap

## Purpose

This roadmap advances eggserve from a hardened static-server release candidate to a production-grade, read-only HTTP/1.1 static origin on supported Unix and Windows systems.

The work is deliberately bounded. Eggserve remains:

- a hardened static file server;
- a Rust library for HTTP, path-confinement, response-planning, server-lifecycle, and bounded-body primitives;
- a Python package exposing equivalent low-level primitives and an embeddable Rust-owned server runtime.

The work does not turn eggserve into an application server, framework, proxy, or edge platform.

## Scope firewall

The following remain explicit non-goals throughout Plans 060–073:

- in-tree ASGI or WSGI adapters;
- application routing or framework routing;
- middleware stacks;
- CGI, template execution, or dynamic file execution;
- uploads, WebDAV, or any write-capable serving API;
- reverse proxying or upstream load balancing;
- sessions, cookies, authentication frameworks, or authorization policy engines;
- automatic ACME, certificate renewal, or certificate orchestration;
- HTTP/2, HTTP/3, WebSocket, CONNECT, or protocol upgrades;
- virtual hosting, SNI certificate routing, or multi-tenant edge behavior;
- turning the experimental client into a requests/httpx replacement;
- client pooling, redirects, cookies, proxy support, or application-oriented convenience layers;
- ASGI scopes, WSGI environments, lifespan events, middleware conventions, or framework lifecycle semantics.

Downstream projects may build those capabilities using eggserve primitives. This repository only guarantees protocol-neutral building blocks, static-serving behavior, and the server runtime needed to exercise those primitives safely.

## Production profiles

Production readiness is defined by explicit profiles rather than one undifferentiated claim.

### Profile A — Unix reverse-proxy origin

Primary production profile:

- Linux or macOS;
- eggserve bound to loopback or a private interface;
- Caddy, nginx, HAProxy, a cloud load balancer, or equivalent terminates public TLS;
- the edge may provide HTTP/2 or HTTP/3, while the origin remains HTTP/1.1;
- the serving root is operator-controlled and mounted read-only where practical;
- safe defaults remain enabled;
- symlink-following mode is outside the hardened guarantee.

### Profile B — Unix direct HTTPS

Secondary production profile:

- Linux or macOS;
- eggserve terminates TLS using rustls;
- one certificate chain and one key configuration;
- restart-required certificate rotation;
- HTTP/1.1 only;
- no ACME, virtual hosting, OCSP stapling, client certificates, or multi-certificate routing.

### Profile C — Windows reverse-proxy origin

Production only after Plans 062–065 complete:

- supported Windows release on a local NTFS volume;
- pinned root directory handle;
- component-by-component handle-relative traversal;
- all reparse points denied under the hardened profile;
- final files and directories served from already validated handles;
- loopback or private-interface origin behind a mature edge.

### Profile D — Windows direct HTTPS

Production only after both Windows confinement and native TLS qualification complete.

### Functional-only configurations

The following remain outside the hardened production claim until separately qualified:

- Windows SMB/network-share roots;
- Windows non-NTFS filesystems;
- Windows cloud-placeholder or third-party filesystem roots;
- `--follow-symlinks` or any Windows reparse-following mode;
- public plaintext HTTP;
- unrestricted or hostile Python application callbacks;
- experimental HTTP client behavior.

## Central production invariants

### Pinned root identity

The configured root is opened once and retained for the server lifetime. Request resolution is relative to that descriptor or handle. Renaming or replacing the configured pathname must not silently redirect a running server to a different tree.

### Open once, validate once, serve from the validated object

No code path may validate a pathname and later reopen it by name. File metadata, conditional planning, range planning, index probing, directory enumeration, and body streaming must use the validated opened object.

### No-follow hardened profile

Every symbolic-link or reparse-point component is denied by default. Compatibility modes may remain explicit opt-ins but are not covered by the production confinement guarantee.

### Strict HTTP/1 framing

Ambiguous or malformed framing is rejected before service invocation. When message boundaries are uncertain, the connection is closed and may not be reused.

### Bounded remote work

Every remotely controlled resource has a configured bound or deadline, including:

- accepted and active connections;
- TLS handshakes;
- request-line and request-target size;
- header count and aggregate header bytes;
- request-body bytes where generic primitives permit bodies;
- file streams;
- directory-listing entries and generated bytes;
- keep-alive idle duration;
- requests per connection;
- response-write duration;
- graceful-shutdown duration;
- Python callback concurrency.

### Edge separation

Eggserve does not acquire edge-server responsibilities. It must not implicitly trust forwarding headers, provide certificate automation, proxy requests, or implement public client-identity policy. Reverse-proxy deployments should use edge logs for client attribution unless a future, separately scoped trusted-proxy contract is explicitly designed.

## Milestone 5 — Production contract and root identity

Plans 060–061 define the support contract and strengthen the cross-platform filesystem foundation.

### Plan 060

- align README, non-goals, threat model, deployment guide, security policy, release contract, and release criteria;
- define hardened versus functional configurations;
- preserve the downstream-extension model;
- classify stable, provisional, and experimental APIs;
- ensure documentation cannot imply ASGI/WSGI, proxy, or framework scope.

### Plan 061

- pin the serving root descriptor/handle for process lifetime;
- eliminate request-time root reopening;
- make opened-resource ownership explicit;
- ensure index probing, range planning, and streaming do not reconstruct paths;
- add root rename/replacement and descriptor-lifetime tests;
- preserve all existing Unix confinement guarantees.

Milestone exit:

- scope documents and machine-readable criteria agree;
- root identity is pinned on Unix;
- stable APIs are separated from provisional and experimental surfaces;
- no production path reopens a validated resource by name.

## Milestone 6 — Windows hardened filesystem confinement

Plans 062–065 replace parser-only Windows protection with real handle-based confinement.

### Plan 062

- implement a focused Windows feasibility spike;
- prove root-relative component opening from an existing directory handle;
- prove no-follow reparse inspection;
- evaluate `NtCreateFile`/`NtOpenFile`, safe wrapper crates, and minimal audited FFI;
- prove conversion from validated handles to the existing streaming layer;
- produce a go/no-go architecture record before broad integration.

### Plan 063

- introduce a pinned Windows root handle;
- record volume and file identity;
- reject reparse-point roots;
- implement component-relative traversal;
- deny all reparse tags by default;
- retain handles through the complete request resolution.

### Plan 064

- use validated handles for metadata and file streaming;
- implement handle-based index-file lookup;
- implement handle-based directory enumeration;
- preserve dotfile and directory-listing policy;
- prevent pathname reconstruction in Windows response paths.

### Plan 065

- build the Windows adversarial test matrix;
- test symlinks, junctions, mount points, unknown reparse tags, namespace aliases, ADS, reserved names, case aliases, short names, and concurrent replacement;
- define supported filesystems and unsupported environments;
- add real-Windows VM evidence and release gates;
- promote local NTFS from functional to hardened only after evidence closes.

Milestone exit:

- local NTFS has handle-relative no-reparse confinement;
- no response body can originate outside the pinned root;
- all reparse tags are denied by default;
- unsupported Windows roots fail closed or are clearly classified functional-only.

## Milestone 7 — Internet-facing runtime hardening

Plans 066–070 qualify the HTTP runtime for hostile network input without expanding product scope.

### Plan 066

- define and test Caddy and nginx reverse-proxy origin profiles;
- add safe deployment examples and service-manager/container guidance;
- ensure eggserve binds only to intended interfaces;
- prohibit implicit trust of forwarding headers;
- add edge/origin timeout and request-size guidance;
- introduce proxy interoperability test infrastructure.

### Plan 067

- separate admission budgets for accepted sockets, TLS handshakes, HTTP connections, file streams, request bodies, and Python callbacks;
- add keep-alive idle timeout and maximum requests per connection;
- add aggregate header and count limits;
- add graceful-shutdown deadline and deterministic forced shutdown;
- prove permit recovery on every cancellation and error path.

### Plan 068

- construct a reverse-proxy desynchronization harness;
- test TE+CL, duplicate and conflicting Content-Length, invalid chunks, obsolete folding, whitespace anomalies, premature bodies, pipelining, and connection reuse;
- compare edge and origin connection disposition;
- require that malformed framing never reaches a hidden second request.

### Plan 069

- harden native TLS for bounded direct deployment;
- separate handshake timeout and concurrency from HTTP header policy;
- test malformed, stalled, truncated, and aborted handshakes;
- define certificate/key startup validation;
- test shutdown during handshakes and responses;
- retain restart-only certificate rotation and the existing no-ACME boundary.

### Plan 070

- close remaining HTTP/1.0 and HTTP/1.1 conformance gaps;
- extend raw-wire, canonical, and property corpora;
- add stateful live-socket fuzzing over request sequences, body lifecycle, rejection, pipelining, TLS truncation, and shutdown;
- require connection closure after ambiguous framing;
- preserve the static service as GET/HEAD and bodyless by default.

Milestone exit:

- reverse-proxy profiles pass interoperability and smuggling tests;
- every connection phase is bounded;
- native TLS passes handshake-abuse qualification;
- malformed framing cannot produce frontend/backend disagreement or origin desynchronization.

## Milestone 8 — Adversarial and operational qualification

Plans 071–072 convert design confidence into release evidence.

### Plan 071

- build cross-platform filesystem mutation and fault-injection harnesses;
- continuously replace files and directories while serving;
- test root rename, symlink/reparse substitution, truncation, permission loss, deletion, and enumeration races;
- record served file identities and digests;
- fail on any body not originating from the allowed root dataset;
- inject file, listener, logging, TLS, callback, and shutdown failures.

### Plan 072

- run 24-hour or longer mixed-traffic soaks for release qualification;
- track memory, handles/descriptors, tasks, permits, sockets, CPU, latency, and error categories;
- test installed binaries and wheels rather than source-tree-only builds;
- add SBOMs, checksums, provenance, lockfile/toolchain capture, and exact source identity;
- make evidence aggregation fail closed for stale or mismatched artifacts.

Milestone exit:

- no sustained resource growth or permit leakage;
- no filesystem escape under mutation;
- installed artifacts pass production-path suites;
- release evidence is reproducible and tied to the release SHA.

## Milestone 9 — Independent audit and production release

Plan 073 closes the track.

### Plan 073

- conduct an implementation-independent security review;
- focus on Unix and Windows confinement, HTTP smuggling, body lifecycle, response framing, TLS admission, Python FFI lifetime, cancellation, shutdown, and artifact integrity;
- remediate or explicitly bound every finding;
- publish final platform and filesystem support matrices;
- define patch, backport, vulnerability-reporting, and evidence-invalidation policy;
- prohibit an unqualified production-grade claim until all security-relevant gates pass on the release SHA.

Milestone exit:

- no unresolved high-severity findings;
- medium findings are corrected or narrowly documented;
- production profiles are supported by current evidence;
- maintenance policy reruns the correct evidence after security-sensitive changes.

## Plan sequence and dependencies

| Plan | Title | Depends on |
|---|---|---|
| 060 | Production support contract and scope firewall | 059 |
| 061 | Pinned root identity and opened-resource ownership | 060 |
| 062 | Windows handle-relative feasibility spike | 061 |
| 063 | Windows pinned root and component traversal | 062 |
| 064 | Windows handle-based static operations | 063 |
| 065 | Windows adversarial qualification and support matrix | 064 |
| 066 | Internet deployment profiles and proxy harness | 060, 061 |
| 067 | Connection lifecycle and resource-budget hardening | 066 |
| 068 | Reverse-proxy desynchronization qualification | 067 |
| 069 | Native TLS direct-deployment hardening | 067 |
| 070 | HTTP/1 conformance and stateful live-socket fuzzing | 068, 069 |
| 071 | Cross-platform filesystem race and fault injection | 065, 070 |
| 072 | Soak, observability, artifact, and provenance qualification | 071 |
| 073 | Independent audit and production-release closure | 072 |

Windows work and internet-runtime work may proceed in parallel after Plans 060–061, but Plan 071 must integrate both completed tracks.

## Release designation rules

Eggserve may claim a bounded production profile only when the corresponding evidence exists on the exact release SHA.

### Unix reverse-proxy production

Requires Plans 060–061, 066–068, 070–073.

### Unix direct-HTTPS production

Requires Unix reverse-proxy production plus Plan 069 direct-TLS gates.

### Windows reverse-proxy production

Requires Unix reverse-proxy production plus Plans 062–065 and Windows evidence in Plans 071–073.

### Windows direct-HTTPS production

Requires all Plans 060–073.

No completion claim may be inferred from code presence alone. Release criteria must identify required commands, platforms, artifacts, evidence age, invalidation paths, and waiver policy.

## Final definition of production grade

Eggserve is production grade for a stated profile when:

1. The serving root is pinned for server lifetime.
2. Every response is sourced from an already validated opened object.
3. Unix uses descriptor-relative no-follow traversal.
4. Supported Windows roots use handle-relative no-reparse traversal.
5. Every remotely controlled resource has a documented bound.
6. Ambiguous HTTP framing is rejected before service invocation and closes the connection.
7. Reverse-proxy edge and origin parsers cannot be driven into request desynchronization by the qualified corpus.
8. Native TLS, where claimed, is bounded and passes handshake-abuse tests.
9. Filesystem mutation suites demonstrate zero root escape.
10. Soak tests show stable memory, descriptors/handles, tasks, sockets, and permits.
11. Python and Rust primitive compatibility fixtures pass without introducing application-framework semantics.
12. Installed artifacts, not only source builds, pass production-path tests.
13. Release artifacts include exact source identity, checksums, SBOM, and provenance evidence.
14. Independent review has no unresolved high-severity findings.
15. Documentation accurately distinguishes supported, functional-only, compatibility, and experimental configurations.
