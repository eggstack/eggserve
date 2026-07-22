# Plan 088 — Comparative Context (Track L)

## Comparison Target

Python `http.server` — the default stdlib static file server that eggserve replaces.

## Methodology Limitations

A rigorous comparison requires:
- Identical files, clients, network topology
- Same machine, same filesystem, same cache state
- Controlled concurrency and connection reuse

This was **not** a controlled benchmark. The numbers below are for contextual reference only.

## Approximate Python http.server Performance

From published benchmarks and ad-hoc measurements:

| Workload | Python http.server (approx) | eggserve (this baseline) | Ratio |
|----------|---------------------------|------------------------|-------|
| GET 1 KiB | ~300-500 us | 12.6 us | ~25-40x |
| GET 128 KiB | ~500-1000 us | 12.9 us | ~40-80x |
| GET 1 MiB | ~2000-5000 us | 12.3 us | ~160-400x |
| HEAD 1 KiB | ~250-400 us | 12.4 us | ~20-30x |
| Range 16 KiB | ~400-600 us | 27.0 us | ~15-22x |
| 304 Not Modified | ~200-300 us | 11.3 us | ~18-27x |
| 404 Not Found | ~150-250 us | 1.9 us | ~80-130x |
| Dir listing (100) | ~500-1000 us | 229 us | ~2-4x |

## Caveats

1. **Python numbers are approximate** — measured on different hardware, different OS, different cache state
2. **eggserve numbers are handler-only** — no TCP, TLS, or HTTP framing overhead
3. **Python numbers include full HTTP stack** — socket accept, HTTP parsing, response serialization
4. **Cache state differs** — Python benchmarks may have been cold-cache
5. **Security model differs** — eggserve does path confinement, policy enforcement, symlink checking; Python does none of this
6. **Feature set differs** — eggserve has conditional requests, range serving, ETag generation, directory listing security headers

## Key Insight

The latency gap is primarily due to:
1. **Rust vs Python runtime** — no GIL, no interpreter overhead
2. **Tokio async I/O** — non-blocking file operations
3. **No path canonicalization** — descriptor-relative traversal on Unix
4. **Compile-time MIME detection** — `phf` perfect hash vs runtime lookup

## What This Comparison Does NOT Mean

- eggserve is not "400x faster than Python" in all scenarios
- The numbers measure different things (handler latency vs full HTTP stack)
- Real-world performance depends on network, client, filesystem, and workload
- The primary value of eggserve is **security**, not performance

## Recommendation

Do not use these numbers for marketing claims. The comparison is for internal reference only. The primary target is predictable bounded behavior, not maximum headline requests per second (per Plan 088 stop conditions).
