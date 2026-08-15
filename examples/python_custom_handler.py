"""Serve a tiny synchronous custom handler through EggServe's Rust runtime."""

from eggserve.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        responses = {
            "/": (200, b"EggServe custom service\n"),
            "/health": (200, b"ok\n"),
        }
        status, body = responses.get(self.path.split("?", 1)[0], (404, b"not found\n"))
        self.send_response(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def create_server(address=("127.0.0.1", 8000)):
    return ThreadingHTTPServer(address, Handler)


def main():
    with create_server() as server:
        print(f"Serving on http://{server.server_address[0]}:{server.server_address[1]}")
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
