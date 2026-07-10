"""Dynamic health endpoint + static assets using eggserve primitives.

This example shows how to combine a tiny dynamic endpoint with eggserve's
SecureRoot for static file serving. eggserve handles path validation,
policy enforcement, and response planning. The example handles HTTP
acceptance and response writing.
"""

import json
from http.server import HTTPServer, BaseHTTPRequestHandler
from eggserve import SecureRoot, StaticPolicy

# eggserve handles: path validation, confinement, response planning
root = SecureRoot("public", policy=StaticPolicy())


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            # Dynamic endpoint: eggserve is not involved
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok"}).encode())
            return

        # Static asset: eggserve resolves and plans the response
        resource = root.resolve_path(self.path)
        if resource.is_file:
            plan = resource.file.plan_response("GET")
            self.send_response(plan.status)
            for name, value in plan.headers:
                self.send_header(name, value)
            self.end_headers()
            if plan.body_kind == "file_full":
                body = resource.file.body_for_plan(plan)
                self.wfile.write(body.read_all())
        elif resource.is_not_found:
            self.send_error(404)
        else:
            self.send_error(403)

    def log_message(self, format, *args):
        pass  # suppress logs for clean output


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 8000), Handler)
    print("Serving on http://127.0.0.1:8000")
    server.serve_forever()
