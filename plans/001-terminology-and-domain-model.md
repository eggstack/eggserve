# EggServe Terminology and Domain Model

Status: canonical normative language (Plan 292).

This document is normative whenever older plans, docs, or code use overlapping terms. Interim documents MUST use these terms and MUST NOT redefine them locally.

## Planning vocabulary

- **Invariant / Capability / Infrastructure / Polish** — the required work classification (`003-planning-process.md` §3).
- **Subsystem roadmap** — dependency-ordered workstream in `plans/subsystems/`.
- **Milestone implementation plan** — bounded agent handoff in `plans/implementation/<subsystem>/`.
- **Closure record** — evidence gate in `plans/closure/<subsystem>/`; legacy gates live in `release/plan-*.md` (immutable).
- **Registry** — `plans/registry.md`, the compact control surface (links, not duplicates).
- **Corrective pass** — a new plan referencing the original milestone + closure; never a silent amendment.
- Status vocabulary: **proposed / ready / active / blocked / closing / closed / conditionally closed / superseded / archived**.
- Dependency vocabulary: **hard / interface / soft / operational**.

## Product domain

- **ConfinedPath** — validated, normalized request path; the only path form the resolver consumes.
- **SecureRoot / PinnedRoot / RootGuard** — configured root confinement handles (Unix descriptor-relative, Windows handle-relative).
- **StaticPolicy** — filesystem composition policy; the field is `symlinks`, never `follow_symlinks`.
- **StaticService** — plans and renders static responses; authority lives in `eggserve-static`, core keeps a wrapper.
- **Service** — the single `Service::call(Request) -> Response` contract (direct authority: `eggserve-server`).
- **RequestBody** — one-shot body (`read_all` or streaming, once); default policy `Reject`.
- **RequestContext** — the single attachment point (`connection()` + `lifecycle()` + bounded `interim()`); prefer `Request::context()` in new code.
- **ResponsePolicy** — runtime-owned Date/Server/denylist/error representation.
- **OpsContext** — per-runtime observability boundary (`ops.emit(...)`); `Logger::global()` is CLI/frontend-init only.
- **TunnelIo** — bounded single-owner post-handshake transport (`101` H1 / `200` otherwise; denial stays ordinary HTTP).
- **H1ConnectionPolicy / PolicyOwner / AdmissionOwnership / RuntimeRejection** — direct-H1 embedding seams (Plans 278–289); framing and denylist stay runtime-owned.

## Error taxonomy (never conflated)

`PathRejection` (path validation) / `RequestValidationError` (HTTP-level, Python-facing) / `ServerError` (`#[non_exhaustive]`, lifecycle) / `ServiceError` (struct over private kind; inspect via `is_panic`/`is_timeout`) / `RequestBodyError` (`#[non_exhaustive]`, body consumption). `RequestCancellationReason` + `ConnectionOutcome` are `#[non_exhaustive]` — match with wildcard.

## Subsystem names (stable)

`static-confinement`, `direct-h1-runtime`, `tls-identity`, `h2-h3-transports`, `python-facade`, `cli-ops-observability`, `proxy-metadata`. New implementation plans use exactly one of these (or propose a new one via the subsystem README + registry update).
