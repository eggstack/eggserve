# Native H1 summary

The full 3-trial native harness captures were `/tmp/eggserve232-native-64.json`
and `/tmp/eggserve232-native-128.json` during the measurement session. The
tracked machine-readable reduction is `../results.json`.

| chunk | body | concurrency | median RPS | mean p95 ms | mean p99 ms | peak RSS KiB | errors |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 64 KiB | 1 KiB | 1 | 5123.807 | 0.221 | 0.251 | 4364 | 0 |
| 64 KiB | 128 KiB | 16 | 10591.415 | 1.871 | 2.296 | 6688 | 0 |
| 64 KiB | 128 KiB | 64 | 8596.282 | 18.008 | 21.647 | 17184 | 0 |
| 64 KiB | 1 MiB | 1 | 1945.083 | 0.546 | 0.641 | 4424 | 0 |
| 64 KiB | 1 MiB | 16 | 2429.928 | 8.109 | 9.163 | 7196 | 0 |
| 64 KiB | 1 MiB | 64 | 1302.728 | 40.560 | 40.654 | 15344 | 0 |
| 128 KiB | 1 KiB | 1 | 5078.891 | 0.223 | 0.258 | 4388 | 0 |
| 128 KiB | 128 KiB | 16 | 10425.528 | 1.748 | 2.110 | 9008 | 0 |
| 128 KiB | 128 KiB | 64 | 9196.625 | 19.173 | 27.124 | 24164 | 0 |
| 128 KiB | 1 MiB | 1 | 2583.432 | 0.461 | 0.527 | 4636 | 0 |
| 128 KiB | 1 MiB | 16 | 2920.044 | 7.647 | 8.455 | 9856 | 0 |
| 128 KiB | 1 MiB | 64 | 1644.804 | 27.559 | 27.801 | 23556 | 0 |

Additional required cases: 16 MiB/concurrency-16 median RPS was 195.073 at
64 KiB and 229.933 at 128 KiB, with zero errors in three trials. Exact 64 KiB
and 512 KiB ranges at concurrency 1/16/64 were also all successful in three
trials each.

