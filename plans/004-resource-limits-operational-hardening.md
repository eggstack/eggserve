# Plan 004: resource limits and operational hardening

## Goal

Turn the static-serving MVP into a predictable, production-oriented service by adding explicit resource limits, timeout behavior, slow-client resistance, sanitized logging, graceful shutdown, and operational defaults. This milestone is where eggserve starts to differ materially from Python's standard `http.server` in production safety.

The implementation should remain narrow. Do not add broad application-server features. The objective is bounded behavior under hostile or accidental load.

## Scope

In scope:

```text
connection concurrency limit
in-flight request limit if distinct from connection limit
file-serving permit/semaphore
header/request-target limit enforcement where Hyper permits configuration
read/header timeout
write timeout or response body timeout strategy
idle keep-alive timeout
request body rejection policy
slow-client tests
large-file concurrency tests
sanitized text logs
optional JSON log mode if dependency cost is acceptable
graceful shutdown behavior
startup effective-policy display update
```

Out of scope:

```text
rate limiting by IP as a required feature
metrics endpoint
admin endpoint
TLS
HTTP/2
reverse proxy support
compression
Range requests
Python public API
```

## Limits model

The `Limits` structure should become enforceable rather than descriptive.

Recommended fields:

```rust
pub struct Limits {
    pub max_connections: usize,
    pub max_in_flight_requests: usize,
    pub max_file_streams: usize,
    pub max_header_bytes: usize,
    pub max_request_target_bytes: usize,
    pub max_request_body_bytes: u64,
    pub header_read_timeout: Duration,
    pub response_write_timeout: Duration,
    pub idle_timeout: Duration,
    pub graceful_shutdown_timeout: Duration,
}
```

If Hyper exposes a server builder setting for a limit, use it. If not, enforce the limit at the accept loop or service layer. Do not leave a configured limit silently unenforced. If a limit cannot be enforced yet, document it and avoid exposing it as stable public configuration.

Suggested initial defaults:

```text
max_connections: 1024 or lower conservative value
max_in_flight_requests: same as max_connections for HTTP/1.1
max_file_streams: 128 or lower conservative value
max_header_bytes: 32 KiB
max_request_target_bytes: 8 KiB
max_request_body_bytes: 0 for GET/HEAD-only server
header_read_timeout: 10s
response_write_timeout: 60s
idle_timeout: 30s
graceful_shutdown_timeout: 10s
```

Exact values can change after testing. The important property is explicit bounded behavior.

## Connection limiting

Use a semaphore around accepted TCP connections:

```text
accept connection
try acquire connection permit
if unavailable, close immediately or briefly reject
spawn connection task
release permit when task exits
```

Avoid unbounded spawning. If the accept loop can outpace the runtime under load, apply backpressure by awaiting permit acquisition before accepting another connection, or close excess connections deterministically.

Document the chosen behavior. For a simple static server, refusing excess connections is acceptable.

## File-stream limiting

Large static files can consume file descriptors and I/O bandwidth. Add a file-serving semaphore:

```text
before opening/streaming file, acquire file permit
release after stream completes or fails
if unavailable, return 503 Service Unavailable or wait briefly according to policy
```

Prefer deterministic failure over unbounded queue growth. If a short wait is implemented, it must have a timeout.

## Request body policy

For the current project scope, request bodies are not useful. `GET` and `HEAD` requests with bodies should be rejected or ignored according to a documented policy. Safer behavior is to reject any request with a non-zero body indication:

```text
Content-Length > 0 -> 413 Payload Too Large or 400 Bad Request
Transfer-Encoding body on unsupported method -> reject
```

Ensure unsupported methods do not allow large bodies to tie up resources. The service should decide method rejection early.

## Timeout policy

Slowloris resistance requires bounding header reads and idle sockets. Hyper configuration and Tokio timeouts should be applied at the connection layer where possible.

Implement:

```text
header/read timeout: client must complete request headers promptly
idle timeout: keep-alive connections cannot linger forever
write timeout: slow readers cannot hold a response stream indefinitely
shutdown timeout: process exits after grace period
```

If write timeout is hard to apply per chunk with Hyper bodies, wrap the file stream or response future. Document any limitations.

## Logging policy

Add sanitized logs. Default text logs are enough; JSON can be optional.

Fields:

```text
timestamp
remote address
method
sanitized path or normalized display path
status
bytes sent when known
duration
error class
```

Sanitization requirements:

```text
escape control characters
truncate extremely long paths
avoid raw terminal escapes
avoid absolute local root path
avoid logging unsanitized query strings by default
```

Do not log raw request bodies. Do not log full local filesystem paths for denied resources. Log categories are more useful than leaking internals.

## Startup policy display

Update startup display to show enforced limits:

```text
Serving root: /path/to/root
Listening: http://127.0.0.1:8000
Methods: GET, HEAD
Directory listing: disabled
Symlinks: denied
Dotfiles: denied
Max connections: 1024
Max file streams: 128
Header timeout: 10s
Idle timeout: 30s
```

If the user binds publicly, print a visible warning or require an explicit public flag in the CLI milestone.

## Graceful shutdown

Implement signal handling in the binary and reusable shutdown support in core.

Behavior:

```text
first shutdown signal stops accepting new connections
existing connections are allowed to finish within grace period
after grace period, remaining tasks are cancelled/dropped
exit status indicates normal shutdown unless fatal error occurred
```

On Unix, handle SIGINT/SIGTERM. On Windows, handle Ctrl-C. Keep this simple and portable.

## Tests

Add tests or harnesses for:

```text
connection limit prevents unbounded accepted tasks
file-stream limit bounds concurrent large transfers
request target larger than limit is rejected
request body on GET/HEAD is rejected according to policy
unsupported method with body does not consume unbounded memory
directory/file responses still work under limits
logs escape control characters and terminal escape sequences
shutdown stops accepting new connections
```

Slow-client tests can be integration tests using raw TCP sockets:

```text
connect and send partial request line slowly
connect and send headers byte-by-byte beyond timeout
connect and read response very slowly
hold keep-alive socket idle beyond timeout
```

These tests should have generous bounds to avoid flakes, but they need to exist.

## Manual validation

Useful manual checks:

```bash
# slow header drip
python scripts/slowloris_probe.py --host 127.0.0.1 --port 8000

# high connection count
hey -c 1000 -n 10000 http://127.0.0.1:8000/file.txt

# large file
curl -o /dev/null http://127.0.0.1:8000/large.bin
```

If helper scripts are added, keep them under `scripts/` and document that they are development probes, not production tooling.

## Acceptance criteria

This milestone is complete when:

```text
Connection concurrency is bounded.
Concurrent file streaming is bounded.
Configured request-target/header limits are enforced or not exposed.
Slow/incomplete requests time out.
Idle keep-alive connections time out.
Request bodies are rejected under current GET/HEAD-only scope.
Logs are sanitized and do not leak local filesystem paths.
Graceful shutdown works on supported platforms.
Tests cover limits and timeout behavior.
```

## Review checklist

Before merging, verify:

```text
No unbounded spawn loop remains around accepted connections.
No configured limit is silently ignored.
No raw terminal-control path is printed in logs.
No metrics/admin endpoint was added opportunistically.
No broad rate-limiter dependency was added without need.
No write/upload functionality was introduced.
Slow-client tests fail before the fix and pass after it.
```

## Handoff notes

After this milestone, eggserve should have a credible production-safety story for static serving over HTTP. The next milestone should focus on CLI ergonomics and Python wheel distribution, not additional server features.
