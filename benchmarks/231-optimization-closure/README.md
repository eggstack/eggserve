# Plan 231 — Performance optimization qualification closure

This directory records the post-implementation qualification for Plans
228–230. The baseline is the Plan 227 capture at
`4fda8a3872684a0f5d1a6ca80de30c452f135080`; the candidate is commit
`bdb247907a2e565e80ecb0c623d630a53264cbf2`. Both were measured on the same
Linux x86_64 host with the same lockfiles and release profile.

Reproduction commands:

```sh
cargo build --release --locked -p eggserve-bin
cargo build --release --locked --example streaming_service -p eggserve-core
python3 benchmarks/227-current-head/native_harness.py --trials 3 \
  --output benchmarks/231-optimization-closure/raw/native-candidate-static-results.json
python3 benchmarks/170-closure/benchmark.py --trials 3 --skip-tls \
  --output benchmarks/231-optimization-closure/raw/plan170-candidate-results.json
cargo test --release --locked -p eggserve-core \
  --test plan227_body_benchmark -- --ignored --nocapture
```

The captured candidate outputs are raw evidence, not timing gates. All native
and custom-service cases completed with zero errors. Static 1 MiB throughput
improved materially under the selected 128 KiB default; custom buffered and
streaming paths remained within normal same-machine variance. The body/frame
matrix and deterministic resource tests are retained alongside the network
captures.

Candidate syscall summaries are in `profiles/`; the baseline summaries remain
in `../227-current-head/profiles/`.

`perf` remained unavailable because this host reports
`perf_event_paranoid=4`; the Plan 227 `strace -f -c` summaries are the
supporting syscall profile. No allocation counts are claimed.

Plan 232 is the corrective follow-up to this closure. The captured candidate
run above intentionally used `--skip-tls`; the explicit file-read target,
forced-over-capacity regression, 64/128 KiB live comparison, range probes, and
representative TLS evidence are recorded in
[`../232-corrective/`](../232-corrective/). The original raw evidence is
unchanged.
