"""Safe file download handler using eggserve primitives.

This example resolves user-provided download names through SecureRoot
to prevent path traversal. It distinguishes not-found from denied and
uses the response planner for metadata. Never join user paths directly
to a filesystem root.
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import unquote
from eggserve import SecureRoot, StaticPolicy

# eggserve enforces: path confinement, symlink denial, dotfile denial
root = SecureRoot("downloads", policy=StaticPolicy())


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        # Extract the filename from the URL path
        name = unquote(self.path.lstrip("/"))
        if not name:
            self.send_error(400, "No file specified")
            return

        # Resolve through SecureRoot — never join paths manually
        resource = root.resolve_path(name)

        if resource.is_file:
            plan = resource.file.plan_response("GET", dict(self.headers))
            self.send_response(plan.status)
            for n, v in plan.headers:
                self.send_header(n, v)
            self.end_headers()
            if plan.body_kind == "file_full":
                with resource.file.into_std_file() as f:
                    self.wfile.write(f.read())
        elif resource.is_denied:
            self.send_error(403, "Access denied")
        else:
            self.send_error(404, "File not found")

    def log_message(self, format, *args):
        pass


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 8000), Handler)
    print("Download server on http://127.0.0.1:8000")
    server.serve_forever()
