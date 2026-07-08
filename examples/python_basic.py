"""Minimal eggserve Python API example.

This example demonstrates the safe-by-default Python API for serving
static files. It is NOT an ASGI/WSGI server or a request callback system.
"""

from eggserve import ServeConfig, ServerProcess, StaticPolicy, serve_directory


def main():
    # Simplest usage: serve current directory on 127.0.0.1:8000
    # This blocks until interrupted (Ctrl+C).
    # serve_directory(".")

    # With explicit config:
    config = ServeConfig(
        directory="public",
        bind="127.0.0.1",
        port=9000,
        policy=StaticPolicy(directory_listing=True),
    )
    proc = ServerProcess(config)
    proc.start()
    print(f"Server started on PID {proc.pid}")
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.stop()
        print("Server stopped")


if __name__ == "__main__":
    main()
