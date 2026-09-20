# Plan 233 — Performance evidence provenance and closure polish

Evidence-only follow-up to Plan 232. No production code, default,
dependency, API, or tier changed under this plan.

## Why this exists

Plan 232 closed with three provenance/reporting gaps:

1. its README recorded the implementation-SHA CI run
   (`380e5dc4a04b5596e58a678fef51ba701a17596e`, run `35483634791`)
   while the subsequent documentation-closure SHA
   (`523e84197fffc9b2ca5834580c7b7d696f86901c`) also passed CI
   (run `35484285932`) without that stronger final-state evidence
   being recorded;
2. the full three-trial native and TLS captures lived under `/tmp`
   with only compact reductions committed, so individual trial
   values were not independently auditable;
3. range correctness was recorded as exact but without the
   per-range throughput, latency, RSS, and resource fields the
   plan requested.

This directory closes those gaps with retained per-trial JSON.

## Layout

```text
benchmarks/233-evidence-polish/
  README.md                  # this file
  results.json               # aggregate derived mechanically from raw/ (see aggregate.py)
  harness.py                 # benchmark-only stdlib harness: native + range + TLS capture
  environment.py             # benchmark-only stdlib host/lockfile/command capture
  aggregate.py               # benchmark-only stdlib aggregate builder (min/median/max, no invented CI)
  raw/
    native-64k-trials.json
    native-128k-trials.json
    ranges-64k-trials.json
    ranges-128k-trials.json
    tls-established-trials.json   # final 128 KiB default only
    tls-handshake-trials.json     # final 128 KiB default only
    environment.json
  ci/
    plan232-closure-ci.json       # run 35484285932, all three jobs successful
    plan233-closing-ci.json       # exact-SHA closing run, recorded post-CI
```

No profiler captures, certificates, private keys, sockets, or temporary
working directories are committed. The TLS certificate was an ephemeral
local RSA-2048 (`openssl req -x509 -newkey rsa:2048 -nodes -days 1
-subj /CN=localhost`); only the generation command is retained.

## Reproduction

```sh
cargo build --release --locked -p eggserve-bin --features tls
python3 benchmarks/233-evidence-polish/harness.py \
  --chunk-label 128k --chunk-bytes 131072 \
  --output-dir benchmarks/233-evidence-polish
python3 benchmarks/233-evidence-polish/harness.py --tls-only \
  --chunk-label 128k --chunk-bytes 131072 \
  --tls-cert /tmp/<ephemeral>/cert.pem --tls-key /tmp/<ephemeral>/key.pem \
  --output-dir benchmarks/233-evidence-polish
python3 benchmarks/233-evidence-polish/environment.py \
  --base-sha <HEAD> --diff-sha256-64k <diff> --trials 3
python3 benchmarks/233-evidence-polish/aggregate.py
```

The 64 KiB comparison used the same release build and harness after a
temporary source-only default override
(`DEFAULT_STREAM_CHUNK_SIZE = 64 * 1024` in
`crates/eggserve-server/src/runtime_limits.rs`, one line), identified by
its diff hash in each 64 KiB trial file and in `raw/environment.json`.
The tree was restored to the 128 KiB default and rebuilt before the
final evidence was committed; `git diff` for the closing SHA contains no
production-source change.

Method per case: one excluded warm-up run, then three measured trials
(`--trials 3` enforced, minimum 3). Native requests use the
dependency-free Rust client from
`benchmarks/227-current-head/native_client.rs`; range and TLS requests
use CPython stdlib `http.client` with threading. One server process
serves all cases of a regime; the `peak_rss_kb` field is therefore the
process high-water mark reached up to and including that trial
(cumulative, retained as observed — same convention as Plan 232), which
still orders the regimes: the 64 KiB run's high-water mark stays below
the 128 KiB run's. Server limits for every run: `--max-connections 512
--max-file-streams 512 --max-in-flight-requests 512 --max-buf-size 65536
--max-headers 100 --max-header-bytes 32768 --max-request-target-bytes
8192`.

## Results and decision

All native, range, and TLS trials completed with zero errors. The
reproduced evidence supports the Plan 232 decision, so the 128 KiB
default stands with no new corrective plan:

- 1 KiB responses remain roughly neutral across regimes;
- 128 KiB is materially faster for 1 MiB static bodies (about 16–30%
  higher median RPS at concurrency 1/16/64) and for 16 MiB at
  concurrency 16 (about 13% higher);
- 64 KiB retains lower high-concurrency peak RSS (for example 1
  MiB/concurrency-64 peaks at about 26 MiB versus 34 MiB), still
  bounded by `max_file_streams * stream_chunk_size`;
- every range trial (64 KiB and 512 KiB ranges at concurrency 1/16/64,
  both regimes) returned status 206 with exact `Content-Length`,
  exact `Content-Range`, and byte-exact bodies;
- TLS established keep-alive (1 KiB/1 MiB at 1/16/64) and 48-connection
  handshake churn (three trials, all 48/48 successful) match the Plan
  232 ballpark on this host.

One noted deviation: the 128 KiB-body/concurrency-16 cell measured
faster under the 64 KiB regime this session (about 22% median RPS),
unlike Plan 232's neutral reading for that cell. Trial spreads are
tight within each regime, so this is same-machine cross-run drift
(regime runs are 30+ minutes apart), not a decision-reversing result:
the 1 MiB/16 MiB advantage and the RSS tradeoff that carry the 128 KiB
decision both reproduced. Absolute timings remain same-machine
evidence, never CI gates — the 168-qualification snapshot shows why
two runs an hour apart can differ ~1.7x with zero errors either way.

## Provenance relationship

- Plans 227/231/232 evidence is immutable; this plan only
  cross-references it.
- `benchmarks/232-corrective/README.md` records the implementation run
  and now cross-references both the closure-SHA run and this directory.
- `ci/plan232-closure-ci.json` preserves run `35484285932`
  (closure SHA `523e841...`, Rust/Python/supply-chain all successful).
- `ci/plan233-closing-ci.json` preserves the exact-SHA closing run
  for this plan's own commit.
- `results.json` is a summary only: every row traces to retained raw
  trial rows via `aggregate.py` (report spread as min/median/max;
  no confidence intervals are invented from three trials).

SHA note: the verified implementation/evidence SHA is
`e40827a60ca744193b8e5ade4ce57464bdbafe94` (run `35489108255`,
Rust/Python/supply-chain successful). The `plan233-closing-ci.json`
record itself landed in a later non-semantic metadata commit that
changed no evidence or documentation content; that commit received
its own successful CI run rather than another in-repo record, which
terminates the record-CI-then-new-SHA loop by construction.
