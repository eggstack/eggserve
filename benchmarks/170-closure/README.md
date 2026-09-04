# Plan 170 performance evidence

This directory contains the final, same-machine performance evidence for the
production/embedding roadmap. The numbers are reproducible baselines, not
release gates or a leaderboard. Absolute RPS and latency must not be used as
CI assertions or copied into marketing prose.

The standard-library harness records source SHA, `Cargo.lock` hash, build
profiles, machine metadata, runtime limits, request/reuse policy, latency
percentiles, errors, CPU time, RSS, fd/thread counts, and trial spread. It
covers native static HTTP/1 scaling, native buffered and streaming services,
installed-wheel `eggserve.lowlevel`, admission saturation, and a same-session
CPython `http.server` substitution baseline. TLS established-connection and
new-connection/handshake workloads are separate.

## Reproduction

From the repository root, build the final code used by the capture:

```sh
cargo build --release --locked -p eggserve-bin
cargo build --release --locked --example streaming_service -p eggserve-core
```

For the Python cases, build and install the wheel into a temporary virtual
environment using the repository's normal wheel flow. The interpreter passed
to the harness must import the installed wheel; source-checkout imports are
not evidence:

```sh
wheel_dir="$(mktemp -d)"
python3 -m maturin build --profile dist --interpreter python3 -o "$wheel_dir"
python3 -m venv /tmp/eggserve-170-venv
/tmp/eggserve-170-venv/bin/pip install "$wheel_dir"/*.whl
```

Run with an existing test certificate and key for the TLS cases (the harness
does not generate or manage certificates):

```sh
python3 benchmarks/170-closure/benchmark.py \
  --python /tmp/eggserve-170-venv/bin/python \
  --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

The capture uses three measured trials plus an excluded warm-up. Use
`--output` to preserve multiple sessions rather than replacing the tracked
baseline. A run without `--python` or TLS credentials records those families
as not run.

The caller-owned seam is a separate in-process microbenchmark because it does
not bind a socket:

```sh
EGGSERVE_BENCH_ITERATIONS=10 cargo test --release --locked \
  -p eggserve-core --test transport_benchmark -- \
  --ignored --nocapture
```

It measures buffered and known-length streamed responses over
`tokio::io::duplex`, using the same public `Service` and canonical connection
driver as TCP/TLS. Its output is not comparable to network RPS; record it as
embedding-seam evidence alongside `results.json`.

Linux x86_64 is the required closure platform. The captured session did not
have an arm64 host available, so arm64 remains performance-unqualified while
the existing correctness/support qualification remains unchanged.

## Interpretation

The results establish profile-specific, same-method baselines and verify
protocol correctness, bounded resource behavior, and admission recovery.
They support narrowly worded claims about representative native scaling,
bounded streaming, the low-level Python substrate, caller-owned transport,
TLS overhead, and static migration behavior. They do not establish universal
performance superiority, edge-server parity, DDoS resistance, anonymity, or
ASGI/WSGI/Gunicorn/Granian compatibility.
