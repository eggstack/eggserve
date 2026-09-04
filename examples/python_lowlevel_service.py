"""Serve a tiny bounded application service through ``eggserve.lowlevel``.

Demonstrates the public handler-only runtime/service substrate: no static
root, no ``http.server`` facade classes. The Rust runtime owns sockets,
parsing, framing, timeouts, admission, and shutdown; the synchronous
``handler`` callable receives canonical requests and returns canonical
responses. ``/stream`` returns a bounded unknown-length streamed response
(chunked framing is selected by the runtime, never by the handler).

This blocks until Ctrl+C. It binds to loopback with safe defaults.
"""

import time

from eggserve import lowlevel


def handler(request):
    path = request.path.split("?", 1)[0]
    if path == "/health":
        return lowlevel.Response.text(200, "ok\n")
    if path == "/stream":
        return lowlevel.Response.stream(200, [b"tick ", b"tock"])
    if path == "/":
        return lowlevel.Response.text(200, "EggServe low-level service\n")
    return lowlevel.Response.text(404, "not found\n")


def create_server(address=("127.0.0.1", 8000)):
    host, port = address
    config = lowlevel.RuntimeConfig(bind=host, port=port)
    return lowlevel.Server(config=config, handler=handler)


def main():
    server = create_server()
    server.start()
    server.wait_ready()
    print(f"Serving on http://{server.addr}")
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass
    finally:
        server.shutdown()
        server.wait()


if __name__ == "__main__":
    main()
