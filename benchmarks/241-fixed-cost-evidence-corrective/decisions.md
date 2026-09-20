# Plan 241 decisions

Every newly measured workload is classified as required by the corrective
plan. These are profile-specific same-host decisions, not universal
performance claims.

| Workload | Decision | Evidence and interpretation |
| --- | --- | --- |
| Custom H1, 1 KiB, c1/c16/c64 | CONFIRMS | Three-trial baseline/candidate native records have zero errors and correct 1 KiB responses. Candidate remains within ordinary same-host variance; dispatch cleanup is retained. |
| HEAD known-length static file | CONFIRMS | Three-trial status/body/header checks pass for both SHAs. |
| Conditional 304 | CONFIRMS | Validator-derived 304 responses have empty bodies and zero errors for both SHAs. |
| Satisfiable range | CONFIRMS | 206 status, exact `Content-Range`, length, and bytes pass for both SHAs. |
| Short/nested/query/percent-encoded paths | CONFIRMS | All response-shape cases pass exact body/status checks; nested path traces retain owned intermediate descriptors. |
| Root directory/resource | CONFIRMS | Root resource response is correct for both SHAs. |
| Caller-owned H1 | CONFIRMS | Existing caller-owned duplex example returns normal 200 response for both SHAs. |
| Established TLS H1 | NEUTRAL | TLS 1.3 sessions, metadata-light static responses, reuse, and zero-error behavior are retained; RPS variance is not promoted to a timing claim. |
| TLS handshake churn | NEUTRAL | 48 new connections per trial complete with zero failures; this is a regression check, not per-request metadata evidence. |
| TLS request metadata | NEUTRAL | Installed-wheel TLS callback observes truthful HTTPS/TLS 1.3 fields with zero errors; candidate-specific sharing remains deferred. |
| Peer-certificate-chain exposure | N/A | No deterministic mTLS/peer-chain fixture was available; Plan 241 adds no fixture or production surface. |
| Python lazy/accessed compatibility views | CONFIRMS | Isolated baseline/candidate wheels cover all requested callback/view modes with zero handler/request errors and preserved observed values. |
| Python metadata-heavy callback | NEUTRAL | Address, scheme, effective, proxy, and TLS fields remain truthful; metadata sharing is still deferred. |
| Python slow streams, 10/100/120 | CONFIRMS | Thread/fd/RSS growth follows the known one-producer-thread-per-stream model and returns to the steady state after clients close. |
| Python disconnect under backpressure | CONFIRMS | Producer/channel resources return to the steady state without truncation/error evidence beyond the intentional disconnect. |
| Python shutdown with active streams | CONFIRMS | Shutdown completes after clients close during drain; threads/fds return to the stopped state. |
| Unix resolver syscall proof | CONFIRMS | Baseline focused trace shows per-request root-FD duplication; candidate removes it for one-component lookup and retains hardened nested stat/open/type/close operations. |

The Plan 237 metadata-sharing subtrack remains `DEFER`, Plan 238 remains
`NO-GO`, and the Plan 239 producer redesign remains `DEFER`. No result
contradicted retained production behavior, so no production corrective plan was
opened.

## Historical reconciliation

Plan 234's original baseline record retained the static nine-point matrix and
source/profiling observations. The custom H1, response-shape, TLS, installed
wheel callback, slow-stream, and focused before/after syscall measurements
listed above were completed by this Plan 241 corrective; they are not relabeled
as measurements that existed at Plan 234 execution time.

Plan 240's acceptance criteria are therefore reconciled as follows:

- `PASS — completed by Plan 241 corrective`: custom H1, path-specific static
  cases, established TLS/handshake regression, installed-wheel Python callback
  views, Python slow-stream resources, and focused resolver syscall proof.
- `PASS — Plan 240 evidence`: retained native static nine-point matrix,
  deterministic correctness/resource suites, routine conformance, package and
  supply-chain checks, Python wheel suite, and Plan 240 closing CI run
  `35538302042`.
- `N/A — deliberately deferred/no-go`: Plan 237 metadata sharing, Plan 238
  shared request state, Plan 239 producer redesign, and peer-chain exposure in
  the unavailable fixture.
- `BLOCKED`: none.
