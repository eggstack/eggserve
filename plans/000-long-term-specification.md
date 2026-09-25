# EggServe Long-Term Specification

Status: canonical long-term implementation directive (Plan 292).

Companion documents:

- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

This document defines the intended end state for EggServe. Normative user and embedding contracts live in `docs/` and `architecture/`; this file states direction and invariants and points at them rather than duplicating them.

The keywords MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are normative.

## 1. Product definition

EggServe is a hardened, HTTP-correct static file server and reusable Rust HTTP/static-serving library, with a Python `http.server`-shaped facade. Static serving is the primary product. The CLI is static-only; the Python facade adds bounded synchronous custom handlers; `eggserve.lowlevel` exposes a handler-only runtime/service substrate (plus experimental H1-only async); `eggserve-core::server` exposes an experimental low-level Rust service boundary.

EggServe is not an app framework, ASGI/WSGI runtime, CGI/FastCGI executor, proxy, or general `socketserver` replacement. H1 + canonical `primitives` are supported; `server`/H2/H3/tunnel/trailer/adapter/listener/proxy/TLS-identity/async-Python remain experimental (see `docs/non-goals.md`, `architecture/overview.md`).

## 2. Architectural ownership (normative)

- `eggserve-primitives` owns the canonical transport-neutral request/response/body/lifecycle model.
- `eggserve-server` owns the H1 connection runtime and the single `Service` contract (H1 `Auto` classifies before any Hyper service exists; core executes H2 only).
- `eggserve-static` is the SOLE static/path/filesystem authority (parsing, `SecureRoot`, confinement, MIME, planning).
- `eggserve-h3` is the sole QUIC dependency owner (experimental adapter).
- `eggnet-tls` is the neutral rustls identity/trust/client-auth/reload substrate (production graph: `rustls` + `rustls-pki-types` only).
- `eggserve-core` is the compatibility/composition umbrella (facades only, no second implementation).
- `eggserve-bin` is the static-only CLI; `eggserve-python` is the workspace-excluded wheel.

Checked by `scripts/check-crate-topology.py`; see `architecture/crate-topology.md`. Ownership changes require an ADR in `plans/adrs/`, not a silent implementation-plan edit.

## 3. Invariants

1. Safe defaults are never silently overridable: loopback bind, no symlinks, no dotfiles, no directory listing unless the user explicitly opts in (`docs/security-policy.md`).
2. No serving outside the configured root: traversal/symlink escape denied at library level (Unix safe defaults: `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)`; `docs/threat-model.md`).
3. No broad dependencies: every dependency needs an explicit purpose (`docs/dependency-policy.md`).
4. Unsafe Rust denied by default; only the reviewed boundaries in `docs/unsafe-code-policy.md` are allowed.
5. Response framing belongs to the runtime only; EggServe owns Date/Server by default; direct H1 may explicitly transfer successful service-response Date/Server metadata only through the qualified ownership seams.
6. `RequestBody` is one-shot; `Service::call` takes `Request` by value.
7. Library code never `println!`/`eprintln!`; runtime code emits via `OpsContext`.

## 4. Protocol and surface tiers

- HTTP/1.1 is the minimal/default compatibility baseline. H2/H3 are opt-in, feature-gated, and experimental; promotion needs a new scoped plan with independent-client, adversarial, platform, and release evidence (Plans 191–195 precedent).
- Python `http.server`-shaped surfaces stay H1-shaped and text-only unless a separate product decision changes them.

## 5. Non-goals

`docs/non-goals.md` is the controlling non-goal list. A change crossing a non-goal MUST update `docs/non-goals.md` + `docs/threat-model.md` in the same change. Planning-process changes (like Plan 292) do not cross product non-goals.

## 6. Acceptance posture

Routine CI (`verify.sh fast` scope + supply-chain + wheel harness) is a regression screen, not certification. Platform qualification (macOS arm64 + Windows adversarial FS) and publication (crates.io manual, PyPI via OIDC) are separate, explicitly evidenced gates. Performance claims name a `docs/deployment.md` profile plus retained evidence.
