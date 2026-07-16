# Phase 60 — Production Support Contract and Scope Firewall

## Goal

Define the exact production claims eggserve may eventually make, align every public and machine-readable contract with those claims, and prevent the production-hardening track from expanding eggserve into an application server, framework, proxy, or general edge platform.

This phase is a contract and enforcement phase. It should change documentation, release metadata, API classification, consistency checks, and tests. It should not add network features or Windows filesystem implementation.

## Starting state

The repository already provides:

- a hardened static CLI server;
- Rust static-serving and canonical HTTP primitives;
- Python native and server primitives backed by the Rust runtime;
- Unix descriptor-relative no-follow traversal;
- functional Windows support with parser-level restrictions;
- optional native rustls TLS;
- release criteria and evidence aggregation;
- explicit non-goals covering ASGI/WSGI, routing, middleware, reverse proxying, ACME, HTTP/2, and application-framework behavior.

Contract inconsistencies remain:

- Windows reparse-point hardening is currently an explicit non-goal but must become a bounded production work item;
- the project is described in some places as only a development/utility tool despite a production-hardening roadmap;
- “production” is not divided into reverse-proxy, direct-TLS, Unix, Windows, hardened, and functional-only profiles;
- native TLS and reverse-proxy expectations are documented but not represented as precise support tiers;
- the public primitives can enable downstream clients or application servers, but the repository must not imply ownership of those downstream products;
- experimental and stable API boundaries must remain explicit during the new work.

## Non-goals

Do not add:

- ASGI or WSGI adapters;
- routing, middleware, framework lifecycle, application dispatch, or handler composition;
- reverse proxying or upstream configuration;
- HTTP/2, HTTP/3, WebSocket, CONNECT, or upgrades;
- ACME, certificate renewal, multi-certificate routing, or virtual hosting;
- authentication, sessions, cookies, or authorization;
- uploads or write support;
- client pooling, redirects, cookies, proxies, or requests/httpx convenience behavior;
- trusted-forwarded-header behavior;
- new socket ownership APIs unless required by an already accepted static-server profile.

## Track A — Define production profiles

Create one normative support matrix covering:

1. Unix reverse-proxy origin.
2. Unix direct HTTPS.
3. Windows reverse-proxy origin.
4. Windows direct HTTPS.
5. Local development.
6. Functional-only Windows configurations.
7. Explicitly weaker compatibility configurations.

Each profile must specify:

- platform and architecture;
- supported filesystem class;
- expected network binding;
- TLS termination location;
- HTTP version;
- required security defaults;
- whether symlink/reparse following is allowed;
- whether directory listing is within the hardened claim;
- whether Python callbacks are within the profile;
- required release gates;
- explicit exclusions.

Recommended initial contract:

- Linux and macOS with safe defaults: hardened once the new production gates pass;
- Windows local NTFS: hardened only after Plans 062–065 and Windows evidence pass;
- Windows SMB, non-NTFS, cloud-placeholder, and third-party filesystems: functional-only;
- link-following modes: outside the hardened production profile;
- reverse-proxy origin: preferred public deployment;
- direct rustls: supported as a limited HTTP/1.1 static-server deployment, not an edge platform;
- public plaintext HTTP: unsupported production configuration.

## Track B — Align documentation

Update at minimum:

- `README.md`;
- `docs/non-goals.md`;
- `docs/deployment.md`;
- `docs/security-policy.md`;
- `docs/threat-model.md`;
- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `docs/extension-contract.md`;
- `docs/tls.md`;
- `docs/compatibility.md` if relevant;
- contributor/agent guidance where it currently describes scope or platform status.

Required wording outcomes:

- eggserve is a hardened, read-only HTTP/1.1 static file server and a low-level primitive library;
- downstream clients, ASGI/WSGI adapters, and application servers may be built outside the repository;
- those downstream projects are not release deliverables or supported application-serving modes of eggserve;
- Windows hardening is an active roadmap item rather than a permanent non-goal;
- Windows remains functional-only until evidence supports promotion;
- Caddy/nginx/load-balancer termination is the preferred public deployment;
- native TLS is limited and does not imply ACME, virtual hosting, HTTP/2, or edge parity;
- no document uses “drop-in replacement” in a way that implies behavioral identity with unsafe `http.server` defaults;
- no document claims production support without naming the profile.

## Track C — Encode support status in release metadata

Extend `release/criteria.toml` or a separate referenced machine-readable support file so profiles are evaluable.

The data model should include:

- profile identifier;
- status: `unsupported`, `functional`, `candidate`, `supported-hardened`;
- required platform and target;
- required filesystem class;
- required gates;
- excluded flags/configurations;
- evidence maximum age;
- invalidation paths;
- whether waivers are allowed;
- support notes shown in generated documentation.

Do not duplicate truth across multiple files without a consistency validator. Prefer one source of truth and generated or checked derived documentation.

Add validator rules that reject:

- Windows marked hardened while reparse gates are absent;
- direct TLS marked supported while TLS qualification gates are absent;
- public profiles that permit plaintext by default;
- hardened profiles that permit symlink/reparse following;
- unsupported API or platform claims in README tables;
- profile references to nonexistent gate IDs.

## Track D — Reinforce API stability tiers

Audit current Rust and Python exports and classify them as:

- stable confinement/static-serving primitives;
- stable canonical HTTP value types;
- stable static server lifecycle surfaces;
- provisional generic service/request-body surfaces where warranted;
- experimental client surfaces;
- internal transport adapters.

The audit must ensure:

- no ASGI/WSGI vocabulary enters public types;
- no routing or middleware abstractions are added;
- no public API promises application-server cancellation semantics beyond documented generic server behavior;
- Hyper, Tokio channel, PyO3, and platform FFI implementation types remain absent from stable public signatures;
- Python `__all__`, stubs, signatures, and exception hierarchy match classification;
- downstream compile fixtures demonstrate that primitives can be consumed without depending on internal transport types.

A primitive may be sufficiently general to support a downstream application server. That does not make the primitive itself out of scope. The scope test is whether it expresses protocol-neutral HTTP or resource behavior rather than framework/application semantics.

## Track E — Add contract consistency tests

Extend `scripts/check-contract-consistency.py` and its tests to verify:

- profile names and statuses agree across documentation and metadata;
- platform tables agree with release criteria;
- Windows limitation wording is present until promotion criteria pass;
- native TLS limitation wording remains intact;
- non-goals retain ASGI/WSGI, proxy, middleware, routing, ACME, HTTP/2, and write exclusions;
- downstream-extension language remains present;
- no stable API documentation labels the client as requests/httpx-compatible;
- all referenced plan and gate IDs exist;
- generated release checklist contains every production-profile gate.

Use semantic markers or structured sections where possible instead of brittle unrestricted prose matching.

## Track F — Threat-model revision

Expand the threat model for future production profiles:

- remote unauthenticated network attacker;
- slowloris and connection-hoarding attacker;
- malformed HTTP/framing attacker;
- request-smuggling attacker operating through a reverse proxy;
- filesystem namespace attacker able to mutate content within or adjacent to the root according to each profile;
- Windows reparse and namespace attacker;
- resource-exhaustion attacker;
- log-injection attacker;
- malicious or stalled Python callback as a resource/lifecycle concern, not a sandboxed adversary;
- compromised reverse proxy, kernel, privileged local attacker, and malicious operator root remain out of scope unless explicitly changed.

Define one central invariant for all hardened profiles:

> No remotely supplied request may cause eggserve to read or serve an object outside the pinned root, and malformed or ambiguous HTTP input must not cause cross-request or frontend/backend message-boundary confusion.

## Required tests

Add or update tests for:

- release criteria schema/profile validation;
- documentation consistency;
- non-goal retention;
- public API classification snapshots;
- Rust external-consumer compile fixtures;
- Python import/signature/stub fixtures;
- generated release checklist output;
- invalid metadata fixtures covering false Windows or TLS promotion;
- stale profile evidence rejection.

## Required evidence

Record:

- changed contract files;
- profile table generated from the machine-readable source;
- contract validator output;
- API snapshot diff;
- release checklist diff;
- CI run on the final phase SHA.

No production profile is promoted in this phase. This phase only defines promotion criteria.

## Acceptance criteria

- All public documents describe the same product scope.
- Windows hardening is removed from permanent non-goals but Windows remains functional-only.
- Application serving, ASGI/WSGI, routing, middleware, proxying, ACME, HTTP/2, and write support remain explicit non-goals.
- Downstream use of primitives for clients or application servers is explicitly allowed but not owned by eggserve.
- Every production claim names a profile.
- Profile status is machine-readable and validated.
- API stability tiers remain explicit.
- Contract-consistency tests fail on scope drift or unsupported promotion.

## Stop conditions

Stop and document rather than broadening scope if:

- a proposed production requirement inherently requires reverse proxying, virtual hosting, or application routing;
- an implementation request introduces ASGI/WSGI or framework semantics into stable primitives;
- a profile cannot be represented without ambiguous security guarantees;
- platform status cannot be backed by a concrete gate and evidence path.

## Handoff

Plan 061 may begin after this phase establishes the normative root-identity invariant and hardened profile definitions. Windows implementation must not begin with a false `supported-hardened` status already published.
