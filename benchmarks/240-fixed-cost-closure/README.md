# Plan 240 fixed-cost performance closure

This directory records the candidate closure for Plans 234–240. The candidate
is commit `af9727870236e684746858bc32eb37aa22892251`; the comparison baseline
is commit `504c3d31f46399d361e57a5bab51a6325a0f4acd`. Both use the same release
profile, runtime limits, native Rust client, host, and root lockfile as the
Plan 234 baseline. The Python lockfile is also retained in the machine-readable
record for the excluded crate boundary.

The native matrix has three measured trials after one excluded warm-up for
each case, nine static keep-alive cases, and zero client errors. It is
same-machine evidence only; the small RPS differences are not CI thresholds
and do not support universal performance claims. `strace -f -c` captures are
retained as the profiling fallback because this host has
`perf_event_paranoid=4` and no supported heap/allocation profiler installed.

The closure matrix is intentionally explicit about coverage:

- native static keep-alive: measured A/B and syscall fallback profiles;
- request-target/body/header, static resolver/path, H1 dispatch, and resource
  behavior: deterministic tests plus source-mechanical review;
- Python bridge: canonical-view behavior and excluded-crate compile checks;
  no claim is made here for a new wheel throughput score;
- TLS, H2/H3, platform qualification, and installed-wheel Python: retained
  CI/manual qualification tracks, not inferred from the native matrix.

See `results.json` for machine-readable provenance and `decisions.md` for the
keep/revert/defer decisions.

Plan 241 corrective reconciliation: the narrower-than-written custom H1,
path-specific, TLS, installed-wheel Python, slow-stream, and direct syscall
acceptance items are completed in
`benchmarks/241-fixed-cost-evidence-corrective/`. Plan 238 remains NO-GO;
metadata sharing and the synchronous producer redesign remain DEFER. The
original Plan 240 capture remains the source for its own nine-point native and
deterministic qualification claims.
