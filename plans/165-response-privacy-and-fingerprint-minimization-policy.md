# Plan 165 — Response Privacy and Fingerprint-Minimization Policy

## Status

**READY FOR IMPLEMENTATION.**

Prerequisites: Plan 161. Prefer Plan 163 first so the policy applies identically to TCP/TLS and caller-owned transports. Coordinate response finalization changes with Plan 162.

## Goal

Make final origin-response metadata an explicit EggServe policy so a caller can minimize gratuitous server/host fingerprint signals without bypassing canonical HTTP framing or inventing I2P-specific types in the core.

The primary new consumer is an embedded anonymity-sensitive origin, but the API must remain generic and useful for ordinary hardened deployments.

This plan does **not** claim to make an HTTP server un-fingerprintable. Parser behavior, timing, application content, cache validators, transport behavior, and protocol choices can all remain distinguishing signals. The goal is narrower: remove unnecessary implementation/version/host metadata and make the remaining behavior deliberate.

## Current baseline

EggServe already has useful properties:

- `Server` is absent by default and the final runtime boundary removes application-supplied `Server` before optionally inserting configured identification;
- canonical error bodies are generic and do not serialize internal exception details;
- hop-by-hop/framing headers are runtime-owned;
- a single authoritative `Date` is currently generated from `SystemTime::now()` at final response construction.

The last item is a stable documented invariant today and requires an explicit compatibility migration.

## Final-boundary policy object

Introduce a small runtime response policy, exact name provisional:

```text
ResponsePolicy
  server_identification
  date_policy
  stripped_response_headers
  error_representation_policy
  static_metadata_policy (where applicable)
```

Apply this policy at the final EggServe-owned response boundary after service/static response construction and canonical metadata normalization but before bytes are emitted.

No service/frontend may bypass it by constructing raw Hyper responses.

## Server identification

Retain the secure default:

```text
Server: suppressed
```

Allow an explicit fixed configured value for ordinary deployments that want it. Do not automatically emit crate version, Rust version, Hyper version, OS, TLS implementation, or Python version.

Application attempts to set `Server` remain subordinate to runtime policy.

## Date policy and RFC semantics

RFC 9110 §6.6.1 requires an origin server with a clock to generate `Date` on 2xx, 3xx, and 4xx responses; an origin without a clock must not generate it.

Reference: https://www.rfc-editor.org/rfc/rfc9110.html#section-6.6.1

Implement three explicit modes:

### Standards default

`SystemClock` (or equivalent): generate the current HTTP-date exactly as EggServe does today. This preserves current compatibility and remains the default for normal deployments.

### Caller-supplied clock/date provider

Allow an embedding runtime to supply the origin time source. The provider must return a valid time value, not an arbitrary header string, so EggServe continues to own formatting/validation.

This is the preferred anonymity-sensitive mode when the embedding environment has a network-adjusted/router clock. I2P itself has strict peer clock-skew requirements, so an I2P router already has a reason to maintain network-consistent time.

Reference: https://www.i2p.net/en/docs/specs/ntcp2/#clock-skew-guidelines

### Explicit suppression

Allow `Date` suppression only as an explicit privacy/interoperability tradeoff. Documentation must state that suppressing Date on 2xx/3xx/4xx from an origin that has a clock is not RFC 9110-conformant.

Do not use a fixed/stale Date or randomized timestamps; those are worse protocol behavior and can themselves fingerprint the server.

### Hyper ownership

Hyper's HTTP/1 builder has its own automatic Date behavior. Once EggServe supports Date suppression/provider selection, explicitly disable Hyper automatic Date generation and make the EggServe policy the sole Date authority. Add tests proving exactly zero or one Date according to policy.

## Generic outbound header stripping

Add a validated denylist of response header names applied at the final boundary.

Requirements:

- names are normalized/validated by canonical header types;
- denylist removal occurs after service response construction so applications cannot re-add stripped identifiers;
- framing/hop-by-hop headers remain governed by canonical normalization regardless of denylist;
- runtime-required headers cannot be removed when doing so would make the response invalid/ambiguous;
- duplicate occurrences of a denied header are all removed;
- do not expose an unreviewed wildcard that can strip arbitrary protocol-critical fields.

Provide a conservative built-in anonymity-sensitive preset that removes obvious implementation-identification fields such as:

- `Server`;
- `X-Powered-By`;
- additional configured project/framework-specific identification fields.

Do not blindly remove all `X-*`, cache headers, CSP/security headers, CORS headers, `Content-Type`, or application metadata. The caller can extend the explicit denylist.

I2P's HTTP tunnel documentation treats HTTP header normalization/stripping as part of its privacy boundary; that is supporting precedent, not a header list to copy verbatim because those documented fields primarily concern client requests.

Reference: https://www.i2p.net/en/docs/api/i2ptunnel/

## Static metadata privacy

Static-file metadata can disclose host/content characteristics even when the server implementation is hidden.

Current weak ETags are generated from file size and modification time (including nanoseconds). `Last-Modified` also exposes filesystem/content timestamp information.

Add a static-response metadata policy that can independently control:

- `Last-Modified` emission;
- metadata-derived ETag emission.

Do not remove these from normal/default serving; they are useful HTTP validators and performance features.

For the anonymity-sensitive profile choose and document one conservative default after compatibility/performance testing:

1. suppress metadata-derived ETag and optionally Last-Modified; or
2. replace the ETag strategy with a stable opaque/content-derived validator only if it can be implemented without unbounded startup/read cost.

Do not add content hashing merely for theoretical fingerprint resistance without measuring the cost. Suppression is preferable to a complex cache if the profile can accept weaker conditional caching.

When Last-Modified is retained, continue RFC rules that it must not be later than Date.

## Canonical error representation

Preserve generic, bounded, version-independent client errors.

Define the built-in error profile explicitly:

- fixed status-specific plain-text bodies or a deliberately empty-body variant;
- no crate/server version;
- no OS/path/process/Python exception information;
- no generated HTML template with implementation-specific branding;
- fixed Content-Type when a body is emitted;
- HEAD suppression remains correct.

For the anonymity-sensitive preset prefer the existing minimal generic shape unless tests show an empty representation materially improves consistency. Do not create unusual per-error randomness; unstable errors are themselves a fingerprint.

Service-level custom 4xx/5xx responses remain application content and can intentionally reveal application information. The policy should scrub configured headers but must not rewrite arbitrary application bodies unless the error was generated by the EggServe runtime itself.

## Header ordering/casing

Do not promise wire-level imitation of nginx/Apache or randomize header order/case. That creates complexity without removing fingerprintability.

During Plan 168, capture wire snapshots so EggServe's own runtime-generated response shape is known and stable enough to detect accidental version leaks. Treat order/case as an observed implementation property unless a standards/correctness reason makes it contractual.

## Logging and diagnostics audit

The anonymity-sensitive profile concerns client-visible metadata first, but embedding must not create accidental reflection paths.

Audit runtime errors to ensure:

- client responses never contain sanitized log messages or service error text;
- hostile header/target values are bounded/sanitized before logs;
- no transport peer metadata is copied into response headers automatically;
- internal logging may retain bounded operational diagnostics, but documentation distinguishes local-log privacy from network-response privacy.

Do not remove useful local diagnostics solely to make the wire generic.

## Profile definition

Add a documented generic profile such as `AnonymitySensitive` or `MinimalFingerprint` composed from ordinary config fields. The name must not imply cryptographic anonymity.

Suggested defaults:

- Server suppressed;
- caller-supplied network-adjusted Date provider when available, otherwise standards SystemClock unless operator explicitly chooses suppression;
- obvious implementation-identifying response fields stripped;
- minimal canonical runtime errors;
- conservative static timestamp/metadata policy;
- stricter generic resource limits from Plan 164.

The I2P documentation should state that the router is expected to supply a separate WAF/rate-limiting layer. EggServe's profile is origin resource/privacy hardening, not network abuse prevention.

## Frontend exposure

Rust gets the complete policy first.

CLI/Python facade exposure should be intentionally narrower:

- normal CLI keeps standards-compliant defaults;
- allow explicit server-header suppression/fixed values using existing behavior;
- expose advanced privacy policy only where configuration is understandable and tested;
- Python stdlib facade must not silently diverge from `http.server` semantics just because the Rust runtime has an anonymity profile;
- Python low-level runtime from Plan 166 may expose the full reviewed policy.

## Tests

Add canonical/wire golden tests for:

- default response has exactly one valid Date and no Server;
- fixed Server opt-in;
- caller-supplied time source;
- Date suppression with Hyper auto-Date disabled;
- denylisted application headers cannot survive finalization;
- framing/content headers remain correct despite denylist configuration;
- generic 400/403/404/405/408/413/500/503/504 errors contain no version/path/exception text;
- HEAD error behavior;
- default static ETag/Last-Modified unchanged;
- privacy static metadata policy;
- TCP/TLS/caller-owned transport parity;
- Python facade default semantics unchanged.

Add a regression scan that fails if runtime-generated response fixtures contain known identifiers such as `eggserve/`, Hyper/Rust/Python version strings, build paths, or OS names.

## Non-goals

Do not add:

- fake nginx/Apache fingerprints;
- randomized response metadata;
- TLS/client fingerprint impersonation;
- I2P routing/tunnel policy;
- request-side browser fingerprint normalization beyond existing HTTP correctness;
- per-client rate limiting;
- rewriting arbitrary application response bodies;
- a claim that Date suppression alone prevents deanonymization.

## Acceptance criteria

- [ ] EggServe, not Hyper, is the sole authority for Date insertion/suppression.
- [ ] Normal defaults remain RFC-compatible and preserve current one-Date behavior.
- [ ] An embedding caller can supply a trusted time source without supplying raw Date strings.
- [ ] Explicit response-header denylisting occurs after service construction and cannot break framing invariants.
- [ ] Runtime-generated errors never expose implementation/version/internal details.
- [ ] Static metadata leakage is explicitly configurable for the anonymity-sensitive profile.
- [ ] The profile/threat model clearly says “minimize gratuitous fingerprint signals,” not “un-fingerprintable.”
- [ ] I2P integration requires no I2P-specific type in `eggserve-core`.

## Handoff

Plan 166 exposes reviewed policy controls through the Python low-level runtime where appropriate. Plan 168 validates golden wire behavior and the anonymity-sensitive profile under non-TCP/slow-client stress.