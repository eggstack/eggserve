# Plan 232 — Corrective file-stream and performance evidence

This directory records the corrective evidence for the explicit file-read
bound and the final 64 KiB versus 128 KiB live-network decision. The capture
was made on Linux x86_64 from the Plan 232 working tree, using Rust 1.98.1,
the release profile, the dependency-free Rust HTTP/1 client from
`benchmarks/227-current-head/native_client.rs`, and the Plan 170 standard
library harness for TLS.

The implementation correction is the explicit `chunk_len` argument to
`eggserve-server::adapters::read_file_chunk`. The helper wraps the opened
file in a bounded `AsyncReadExt::take` view; allocator-visible
`BytesMut::capacity()` is never a response-length authority. The focused
adapter tests include a deliberately over-capacity buffer, full-file and
range frame boundaries, truncation, and permit release.

## Reproduction

```sh
cargo build --release --locked -p eggserve-bin --features tls
python3 benchmarks/227-current-head/native_harness.py --trials 3 \
  --output /tmp/eggserve232-native-128.json
python3 benchmarks/170-closure/benchmark.py --trials 3 \
  --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem \
  --output /tmp/eggserve232-tls-128.json
```

The 64 KiB comparison used the same release build and harness after a
temporary source-only default override, then restored the 128 KiB default
before the final build. The TLS certificate was an ephemeral local RSA-2048
certificate generated with:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -subj /CN=localhost -keyout key.pem -out cert.pem
```

The compact raw summaries in `raw/` retain the exact case-level medians and
the full machine-readable decision in `results.json`; absolute timings are
same-machine evidence, not CI gates.

## Results and decision

All plaintext, range, and TLS cases completed with zero errors. 128 KiB was
retained as the default. On this host it was materially faster for 1 MiB
static bodies (about 20–33% higher median RPS than 64 KiB at concurrency
1/16/64) and for the 16 MiB concurrency-16 case (about 18% higher median
RPS). The 64 KiB setting used less peak RSS at high concurrency; for example,
the 1 MiB/concurrency-64 case peaked at about 15 MiB versus 24 MiB for 128
KiB. That increase remains within the bounded default payload budget of
`32 * 128 KiB = 4 MiB` before allocator/runtime overhead, and no leak or
permit-recovery failure was observed. The tradeoff is therefore acceptable
for the selected static-serving profile, with no universal performance claim.

Plan 231's raw compatible candidate run intentionally used `--skip-tls`, so
its TLS, installed-wheel Python, CPython substitution, and admission
saturation sections were marked not-run. Plan 232 completes representative
TLS established-keep-alive and handshake-churn evidence. Installed-wheel
Python, CPython substitution, and admission saturation remain inherited from
Plan 170/correctness CI and were not rerun because this correction does not
touch those code paths.

Remote CI closure SHA and workflow URL are recorded here after the final
commit is pushed.
