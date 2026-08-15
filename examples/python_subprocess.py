"""Demonstrate the optional subprocess lifecycle convenience API."""

from eggserve.subprocess import ServerProcess, ServeConfig


def main():
    process = ServerProcess(
        ServeConfig(directory="examples/site", bind="127.0.0.1", port=8000)
    )
    process.start()
    print(f"Server started on PID {process.pid}; press Ctrl+C to stop")
    try:
        process.wait()
    except KeyboardInterrupt:
        process.stop()


if __name__ == "__main__":
    main()
