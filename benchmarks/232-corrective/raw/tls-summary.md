# TLS summary

The full Plan 170 harness output was captured at
`/tmp/eggserve232-tls-results.json`; the tracked machine-readable reduction is
`../results.json`. The final 128 KiB release build used an ephemeral local
RSA-2048 certificate and rustls.

| body | concurrency | median RPS | mean p95 ms | mean p99 ms | errors |
|---:|---:|---:|---:|---:|---:|
| 1 KiB | 1 | 6871.957 | 0.182 | 0.209 | 0 |
| 1 KiB | 16 | 8170.355 | 3.016 | 7.104 | 0 |
| 1 KiB | 64 | 7557.244 | 13.844 | 50.387 | 0 |
| 1 MiB | 1 | 1058.141 | 1.126 | 1.242 | 0 |
| 1 MiB | 16 | 1209.843 | 24.585 | 26.487 | 0 |
| 1 MiB | 64 | 867.944 | 107.355 | 110.446 | 0 |

Separate 48-connection handshake-churn trials measured 971.230, 1373.525,
and 1491.014 handshakes/s, all with zero errors.

