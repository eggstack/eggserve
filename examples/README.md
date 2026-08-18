# EggServe examples

These small examples are executable demonstrations of the supported product
surfaces. They are intentionally not a tutorial framework or a second policy
reference; see the linked normative documentation for the full contract.

Run the commands from the repository root, or replace `examples/site` with
your own content directory. The examples bind to loopback and retain the safe
defaults: directory listings, dotfiles, and symlinks are denied unless a
command explicitly opts in.

## CLI

### Safe local static server

Serves the tiny fixture below the repository's `examples/site` directory. The
CLI binds to loopback by default, serves `index.html` for `/`, supports GET
and HEAD, and does not list directories or serve `.hidden-example`.

```sh
eggserve --directory ./examples/site
```

This blocks until Ctrl+C. From a source checkout, use
`cargo run -p eggserve-bin -- --directory ./examples/site`.

### Explicit public bind

Public exposure is an opt-in network change:

```sh
eggserve --directory ./examples/site --public --port 8080
```

This does not provide edge TLS, reverse-proxy behavior, or other perimeter
controls. See the [CLI reference](../docs/cli.md) and
[deployment guidance](../docs/deployment.md).

### Explicit directory listing

Directory listing can be enabled separately when compatibility requires it:

```sh
eggserve --directory ./examples/site --directory-listing
```

It remains disabled in the default CLI example.

## Python `http.server` facade

### Static server: `python_http_server_static.py`

Demonstrates the source-familiar `eggserve.server` static facade. It uses the
native static fast path for the stock handler configuration while retaining
loopback binding, no listings, no dotfiles, and no symlink following.

```sh
python examples/python_http_server_static.py
```

The example blocks until Ctrl+C. Its `create_server()` function accepts port
`0`, which is used by the installed-wheel smoke test for GET, HEAD, hidden-file
denial, and clean shutdown. See the [Python API reference](../docs/python-api.md)
and [compatibility contract](../docs/python-http-server-compatibility.md).

### Custom handler: `python_custom_handler.py`

Demonstrates a bounded synchronous `BaseHTTPRequestHandler`: `/health` and
`/` return small explicit responses, and other paths return 404. It uses the
Rust-owned listener and framing boundary; it does not use raw sockets,
coroutines, or unbounded streaming.

```sh
python examples/python_custom_handler.py
```

This also blocks until Ctrl+C. The installed-wheel smoke test constructs it on
port `0`, checks the health and unmatched-path responses, and closes it.

## Python convenience and low-level APIs

### Subprocess lifecycle: `python_subprocess.py`

Demonstrates the optional `eggserve.subprocess.ServerProcess` API. It is a
process-management convenience, not the canonical Python server facade.

```sh
python examples/python_subprocess.py
```

### Hardened download primitive: `python_safe_download.py`

Demonstrates the advanced `eggserve.lowlevel.SecureRoot` and response-planning
primitives for a deliberately small download handler. User-controlled names
are resolved through `SecureRoot`; the example never joins or reopens a
translated path. It uses the default loopback bind and blocks until Ctrl+C.

### HTTPS server: `python_https_server.py`

Demonstrates the `ThreadingHTTPSServer` class backed by the Rust TLS backend.
Requires a PEM certificate and key; the file header shows how to generate a
self-signed certificate for local testing.

```sh
python examples/python_https_server.py
```

### Custom response headers: `python_custom_headers.py`

Shows the `default_content_type` and `extra_response_headers` static metadata
hooks. Extra headers are emitted only on final 200 static responses and cannot
override runtime-owned metadata.

```sh
python examples/python_custom_headers.py
```

## Rust library

The Rust examples are Cargo examples and use only public `eggserve-core` APIs.
They default to `127.0.0.1:8000`, accept an optional bind address as their
second argument, and shut down gracefully on Ctrl+C. Passing
`127.0.0.1:0` makes the operating system choose a free port.

### Static server: `static_server.rs`

```sh
cargo run -p eggserve-core --example static_server -- ./examples/site
```

This uses the built-in confined static service, with safe policy defaults
unchanged. The first argument is the root directory; the optional second
argument is the bind address.

### Custom service: `custom_service.rs`

```sh
cargo run -p eggserve-core --example custom_service
```

This demonstrates a deliberately tiny `service_fn` match on method and path:
`GET /health` returns 200, `GET /` returns a small welcome body, and other
requests return a controlled 404. It demonstrates the transport/service
boundary, not routing, middleware, or an application framework.

### Custom response headers: `custom_headers.rs`

```sh
cargo run -p eggserve-core --example custom_headers -- ./examples/site
```

Shows the `default_content_type` and `extra_response_headers` static metadata
hooks. Extra headers are emitted only on final 200 static responses and cannot
override runtime-owned metadata. Verifiable with `curl -sI`.

### HTTPS server: `https_server.rs`

```sh
cargo run -p eggserve-core --example https_server --features tls -- ./examples/site
```

Demonstrates the `tls` feature with rustls-backed HTTPS serving. Requires a
PEM certificate and key; the file header shows how to generate a self-signed
certificate for local testing. Without the `tls` feature, prints a helpful
message.

### Primitives without a socket: `primitives.rs`

```sh
cargo run -p eggserve-core --example primitives
```

This shows the public security and response-planning primitives without
starting a listener. See the [primitives architecture](../architecture/primitives-api.md)
for the complete API boundary.
