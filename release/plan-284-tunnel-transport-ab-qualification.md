# Plan 284 — Tunnel transport A/B qualification

## Decision

**KEEP the direct opaque transport candidate for the qualified H1 tunnel
path.** It preserves the public `TunnelIo` interface and removes the
intermediate duplex copy. This decision is limited to the direct H1 runtime;
the compatibility H3 path continues to use its existing duplex bridge.

## Method and evidence

The ignored deterministic qualification harness is
`tunnel::tests::tunnel_transport_ab_qualification` in
`crates/eggserve-server/src/tunnel.rs`. It compares the former
`duplex + copy_bidirectional` algorithm with direct `TunnelIo` transport
ownership using identical in-process Tokio duplex endpoints. Each of three
repeats runs 100 fixed-size echo exchanges for 1 KiB, 64 KiB, and 1 MiB
payloads, recording p50/p95/p99 echo latency and aggregate payload throughput.
Raw output is preserved in `release/plan-284-tunnel-transport-ab-raw.txt`.

Environment: Linux x86_64, Intel Core i9-9900K, 16 logical CPUs, rustc
1.98.1, Cargo test profile (`debug`); source baseline was planning SHA
`4c2e5b6e0ce52256b61a96f0d8a0b0d20ca32922`. This is a same-host mechanism
comparison, not a production network performance claim. RSS and process CPU
were not isolated by mode; resource accounting is structural: both designs
retain the tracked tunnel task and handler task in production, while the
direct representation removes the 32 KiB duplex allocation and one copy in
each direction. The benchmark's harness task counts are not runtime task
counts.

Across all nine paired trials, direct transport reduced median echo latency
for each payload size. Aggregate throughput was higher in every pair, with
the smallest advantage in the noisier 64 KiB repeat 3. No latency or
throughput regression appeared in the measured matrix. The clear repeated
improvement plus removal of the intermediate buffer justifies retaining the
candidate; no CI timing threshold is introduced.

Correctness remains independently qualified by
`cargo test -p eggserve-server --test tunnel_upgrade`: immediate post-upgrade
read-ahead is echoed exactly, shutdown cancels an active tunnel, and default
and external tunnel admission retain their expected behavior. The H3 adapter
and its duplex bridge are unchanged.

## Limits

This harness uses in-process Tokio duplex transport, not TCP loopback, and
does not measure peak RSS or CPU time separately. Its numbers are local
qualification evidence only. The repository still requires the normal
cross-platform CI matrix for the final candidate.
