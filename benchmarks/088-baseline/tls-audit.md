# Plan 088 — TLS Overhead Characterization (Track I)

## Audit Scope

Measure plaintext vs TLS overhead for:
- Handshake latency
- Resumed vs new connections
- CPU and memory under handshake churn
- File throughput over established TLS connections
- Connection admission behavior
- Timeout behavior

## Assessment

**TLS benchmarks were not run in this baseline** because the `tls` feature was not enabled for the benchmark build. This is intentional — the plan states TLS characterization is optional for the initial baseline.

### Architecture Review

TLS is implemented via:
- `rustls` for the TLS stack
- `tokio-rustls` for async integration
- `webpki-roots` for certificate verification
- Feature-gated: `tls` feature in `eggserve-bin`, `client-tls` in `eggserve-core`

### Known TLS Characteristics

From the architecture docs (`docs/tls.md`):
- TLS is optional and not enabled by default
- Self-signed certificates are supported for development
- No session resumption is configured (each handshake is full)
- TLS adds ~1 RTT for handshake + encryption overhead per record

### What Would Be Measured

A complete TLS audit would:
1. Benchmark handshake latency (new vs resumed)
2. Measure throughput over TLS vs plaintext for various file sizes
3. Characterize CPU overhead under concurrent TLS handshakes
4. Verify timeout behavior with slow TLS clients
5. Test connection admission under TLS load

### Recommendation

Run TLS benchmarks with `cargo bench --bench file_serving --features tls` in a dedicated measurement session. The current baseline provides plaintext-only numbers for comparison.

## Conclusion

TLS characterization deferred to a dedicated measurement session. The TLS code path is feature-gated and does not affect default (plaintext) performance. Architecture review confirms no additional buffer allocations beyond the encryption layer.
