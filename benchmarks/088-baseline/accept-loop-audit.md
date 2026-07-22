# Plan 088 — Accept-Loop and Task Housekeeping Audit (Track H)

## Audit Scope

Inspect the accept loop and task management for:
- `Vec<JoinHandle>` retention and scanning
- Finished-task collection
- Semaphore acquisition path
- Listener error backoff
- Broadcast receiver creation
- Per-connection state cloning
- TLS acceptor construction
- Task and permit cleanup

## Code Analysis

### Task Management (`server/mod.rs`)

The server uses a `Vec<tokio::task::JoinHandle>` to track spawned connection tasks:

```rust
tasks.retain(|t| !t.is_finished());
```

This scan is O(n) where n is the number of active + recently finished tasks. In steady state with 64 max connections, this is bounded.

**Assessment: Acceptable**
- `Vec::retain` is cache-friendly and O(n) with small n (≤64)
- `JoinSet` would provide O(1) finished-task removal but adds API complexity
- The scan runs once per new connection, not per request
- No measurable overhead at the expected connection rate

### Semaphore Acquisition

Connection admission uses `Arc<Semaphore>::acquire_owned()`:

```rust
let permit = semaphore.clone().acquire_owned().await?;
```

**Assessment: Correct and bounded**
- Tokio semaphore uses atomic operations internally
- No allocation on the acquire path (permit is heap-allocated only on success)
- Bounded by `max_connections` (default 64)

### Listener Error Backoff

Listener errors (e.g., `Accept` failure) use a 100ms sleep before retrying:

```rust
tokio::time::sleep(Duration::from_millis(100)).await;
```

**Assessment: Acceptable**
- Fixed backoff prevents tight error loops
- Could be improved with exponential backoff for persistent errors
- Not a performance concern for normal operation

### Broadcast Receiver Creation

Each connection task creates a shutdown broadcast receiver:

```rust
let mut shutdown_rx = shutdown_rx.clone();
```

**Assessment: Correct**
- `broadcast::Receiver::clone()` is O(1) and cheap
- The receiver is a single allocation per connection
- No contention on clone

### Per-Connection State

Each connection clones:
- `Arc<ServeConfig>` — reference count increment (atomic)
- `Arc<PinnedRoot>` — reference count increment (atomic)
- `Arc<Semaphore>` — reference count increment (atomic)

**Assessment: Correct and cheap**
- Three atomic increments per connection
- No deep copies
- Memory: ~24 bytes of Arc overhead per connection

### TLS Acceptor Construction

TLS is feature-gated (`tls` feature). When enabled:
- `rustls::ServerConfig` is built once at startup
- `tokio_rustls::TlsAcceptor::from(config)` is an `Arc` wrap
- Per-connection TLS handshake uses the shared acceptor

**Assessment: Correct** (not measured in this baseline — TLS feature not enabled)

## Summary

| Component | Assessment | Optimization |
|-----------|-----------|-------------|
| Task scanning (Vec<JoinHandle>) | O(n), bounded by max_connections | Not needed |
| Semaphore acquisition | Atomic, O(1) | Not needed |
| Listener error backoff | Fixed 100ms | Could add exponential backoff (low priority) |
| Broadcast receiver clone | O(1) | Not needed |
| Per-connection Arc clones | 3 atomic increments | Not needed |
| TLS acceptor | Shared Arc, per-handshake | Not measured |

## Conclusion

The accept-loop and task housekeeping are correct and bounded. The `Vec<JoinHandle>` scanning is the only potentially suboptimal component, but with a 64-connection limit, the scan cost is negligible. No changes recommended.
