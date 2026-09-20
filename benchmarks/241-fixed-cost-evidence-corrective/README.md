# Plan 241 — Fixed-cost performance evidence corrective

This directory closes the evidence gaps identified after Plan 240. It is an
evidence-only corrective: no production source, default, dependency, public
API, protocol tier, or stream architecture changed.

The A/B comparison is between the frozen Plan 234 baseline
(`504c3d31f46399d361e57a5bab51a6325a0f4acd`) and the retained production
candidate (`af9727870236e684746858bc32eb37aa22892251`). The later Plan 240
documentation SHA (`5b048cbf66f57957625c9ad8b658635a56ac9593`) and the Plan
241 evidence-content/CI SHAs are recorded separately in `results.json`.

## Reproduction

The native harnesses use the release binaries built from detached baseline and
candidate worktrees. The installed-wheel harnesses install each `cp311-abi3`
wheel into an isolated environment and never import the checkout:

```sh
python3.11 benchmarks/241-fixed-cost-evidence-corrective/harness.py \
  --sha <sha> --cli <release-cli> --custom <streaming-example> \
  --caller-owned <caller-owned-example> --output <raw-json>
python3.11 benchmarks/241-fixed-cost-evidence-corrective/syscall_harness.py \
  --sha <sha> --cli <release-cli> --output <syscall-summary>
python3.11 benchmarks/241-fixed-cost-evidence-corrective/aggregate.py \
  --base-native <json> --candidate-native <json> \
  --base-tls <dir> --candidate-tls <dir> \
  --base-tls-callbacks <json> --candidate-tls-callbacks <json> \
  --base-callbacks <json> --candidate-callbacks <json> \
  --base-streams <json> --candidate-streams <json>
```

Native and TLS captures use three measured trials after one excluded warm-up.
The custom-service track is a 1 KiB canonical bytes response at concurrency 1,
16, and 64. Static correctness/latency cases cover HEAD, conditional 304,
range, short and nested paths, query targets, a percent-encoded safe path, and
the root resource. The caller-owned result is a correctness smoke, not a new
throughput claim.

The TLS capture uses established TLS 1.3 H1 connections plus a 48-connection
handshake-churn regression check. The installed Python wheel capture covers
empty/bytes callbacks, method, text headers, ordered headers, byte header
items, raw target/path/query bytes, and metadata-heavy access over plaintext
and established TLS. Peer-certificate-chain exposure is unavailable in the
selected deterministic fixture and is not inferred.

The synchronous stream resource matrix records 10, 100, and 120 active slow
streams, plus disconnect-under-backpressure and shutdown-with-active-streams.
It intentionally documents the current one-producer-thread-per-stream and
bounded 16-chunk channel model; it does not reopen the deferred producer
redesign.

`strace -f -c` summaries and focused syscall traces retain the Unix resolver
proof. The candidate's ordinary one-component lookup has no per-request
`fcntl(F_DUPFD_CLOEXEC)` root duplication, while nested traversal retains
`AT_SYMLINK_NOFOLLOW`, `O_NOFOLLOW`, post-open type validation, intermediate
descriptor ownership, and close behavior.

There is no supported allocator profiler on this host (`perf_event_paranoid=4`,
no heap profiler installed). Allocation statements remain source-mechanical;
the captures do not invent allocator counts. Absolute timing is not a CI gate.

See `decisions.md` for the workload-by-workload classification and
`results.json` for compact machine-readable provenance. Raw per-trial records
are retained below `raw/`; CI provenance belongs under `ci/`.
