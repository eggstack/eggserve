# Phase 35 — Fuzzing and Property-Test Expansion

## Goal

Exercise parser, planner, URL, header, and body-boundary invariants beyond hand-written cases and retain every discovered failure as a deterministic regression.

## Workstream A — Target inventory

Audit existing `fuzz/` targets and map them to public/security-sensitive boundaries. Add or expand targets for:

- origin-form request-target parsing;
- percent decoding;
- path component validation and normalization;
- reserved-name/platform checks;
- range parsing;
- conditional headers and ETag comparison;
- static response planning;
- response header/status validation;
- client URL parsing;
- client request/header construction;
- body range and bounded-read arithmetic.

Prefer pure functions and compact targets. Avoid network or filesystem setup unless the target specifically tests a filesystem invariant.

## Workstream B — Invariant properties

Assert properties, not only absence of panics:

- accepted paths contain no parent/current components, NUL, or ambiguous separators;
- decoding and normalization never reintroduce rejected traversal;
- path length and numeric conversions cannot overflow;
- planned ranges are within file length;
- `Content-Range` and body length agree;
- HEAD, 204, and 304 plans have no emitted body;
- generated header values contain no CR/LF;
- accepted URLs have supported schemes, non-empty hosts, valid ports, and correct bracketed IPv6 authority;
- fragments never enter request targets;
- bounded reads never allocate or return beyond their cap;
- `FileRange` subreads never exceed the represented range.

## Workstream C — Seed corpora

Seed each target with:

- existing regression tests;
- findings from phases 29–34;
- encoded traversal variants;
- separator mixtures;
- Unicode and percent-encoding cases;
- malformed/overflowing ranges;
- malformed dates and ETags;
- duplicate/conflicting framing headers;
- IPv4/IPv6 authorities;
- query-without-slash and empty-port cases;
- maximum-length and off-by-one inputs.

Keep corpus files reviewable and deduplicated.

## Workstream D — Property tests in normal CI

Add deterministic property tests using generated bounded inputs where this gives fast value without requiring libFuzzer. Candidates include:

- range parser/planner round trips;
- URL parse/authority consistency;
- path parse component invariants;
- response body/header consistency;
- bounded body reads.

Use fixed seeds for reproducibility.

## Workstream E — CI strategy

Create two layers:

1. Normal CI runs all corpus regressions and deterministic property tests.
2. A scheduled/manual fuzz workflow runs short campaigns for all targets and uploads crash artifacts.

Document longer pre-release campaigns with target duration, sanitizer/toolchain requirements, and artifact handling. Fuzz jobs must not block ordinary contributor workflows through excessive duration.

## Workstream F — Failure handling

For every crash or invariant violation:

- minimize the input;
- add it to the corpus;
- add a named deterministic unit/integration regression when practical;
- classify security relevance;
- fix the root invariant rather than only special-casing the sample.

## Likely files

- `fuzz/Cargo.toml`
- `fuzz/fuzz_targets/*`
- core parser/planner tests
- `.github/workflows/fuzz.yml`
- `docs/fuzzing.md` or security-testing documentation

## Acceptance criteria

- Every parser-like security boundary has a fuzz or property-test owner.
- Targets assert meaningful invariants.
- Existing audit regressions seed the corpus.
- Normal CI runs corpus regressions.
- Scheduled/manual fuzz smoke campaigns are documented and runnable.
- No known panic, overflow, out-of-range plan, header injection, or traversal invariant violation remains.
- Crash artifacts have a documented triage path.

## Non-goals

- No distributed fuzzing service requirement.
- No indefinite fuzz jobs on every pull request.
- No performance benchmarking disguised as fuzzing.
