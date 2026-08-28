# Plan 152 — Bug Report Closure (2026-08-28)

## Status

**COMPLETE — 2026-08-28.**

## Scope

Close the confirmed actionable findings in the temporary `bugs.md` report with
minimal changes and no feature additions. The report contains several stale or
non-defects; those are documented here rather than changed. Confirmed work is
limited to response metadata accounting, fallback validation parity, low-level
request-target controls, logging/path bounds, short-read framing errors, and
   focused test/conformance coverage. Low-risk hot-path allocation notes O-01
   through O-03, O-05, and O-07 are included; O-04 requires a buffer-pool
   design outside this minimal patch, while O-06 and O-08 explicitly require
   no code change.

## Planned changes

1. Fix post-suppression response length accounting (B-01), reject empty extra
   values at the static response boundary (B-02), and add regressions.
2. Apply child-component validation in the symlink-following fallback (B-03),
   reject low-level raw target controls consistently (B-04), and add parser
   parity/triple-encoding coverage (B-05/T-02).
3. Make path truncation bounds explicit, turn short file reads into transport
   errors, and bound service-error log messages (B-08/B-09/B-11).
4. Add missing normalization/framing corpus coverage (T-01), consolidate the
   duplicated integration-test body helper (T-05), and apply O-01–O-03/O-05/O-07.
5. Re-run the repository verification gates, remove `bugs.md`, and commit the
   closure on `main`.

## Validation

Validation completed: formatting; workspace Clippy and tests (1,489 passed, 2
ignored); TLS Clippy and tests (136 passed); core doc tests (8 passed); fast
verification including the excluded Python crate locked check; conformance
matrix validation; examples check; both dist builds; and packaged Cargo
verification with `ALLOW_DIRTY=true`. `bugs.md` was deleted after closure.

## Dispositions

B-05’s branch order is already correct; B-06 and B-07 have no current defect;
B-10’s parser correctly rejects malformed multi-colon bind values; B-12 is the
documented connection lifetime policy; B-13 already checks each incoming chunk
before exposing it to a service; B-14’s body-error statuses do not include 405.
T-03 and T-04 are qualification/test-environment suggestions without a
production defect and are not justified for this minimal patch. O-06 and O-08
are explicitly non-issues in the report.
