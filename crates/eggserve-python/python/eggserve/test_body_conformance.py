"""Shared Rust/Python conformance corpus tests for request body behavior.

Consumes conformance/body_corpus.json to verify body policy selection, empty
body handling, fixed-length exact/over-limit, chunked exact/over-limit, one-shot
consumption, GET-with-body rejection, and premature EOF behavior.

Must be run from an installed wheel. Uses raw sockets for wire-level verification.
"""

import json
import os
import shutil
import socket
import tempfile
import time
import unittest
import urllib.request

try:
    from eggserve._native import Response, Server

    NATIVE_AVAILABLE = True
except ImportError:
    NATIVE_AVAILABLE = False


CORPUS_PATH = os.path.join(
    os.path.dirname(__file__),
    "..",
    "..",
    "..",
    "..",
    "conformance",
    "body_corpus.json",
)


def _load_corpus():
    with open(CORPUS_PATH) as f:
        return json.load(f)


def _group(name):
    corpus = _load_corpus()
    return corpus["groups"][name]["fixtures"]


def _wait_for_tcp(addr, timeout=5.0):
    host, port = addr.split(":")
    port = int(port)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return True
        except (ConnectionRefusedError, OSError):
            time.sleep(0.05)
    return False


def _send_raw_request(addr, request_bytes, timeout=5.0):
    host, port = addr.split(":")
    with socket.create_connection((host, int(port)), timeout=timeout) as sock:
        sock.sendall(request_bytes)
        chunks = []
        sock.settimeout(timeout)
        while True:
            try:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
            except socket.timeout:
                break
    return b"".join(chunks)


def _parse_status(data):
    try:
        header_end = data.index(b"\r\n\r\n")
    except ValueError:
        return None
    header_str = data[:header_end].decode("utf-8", errors="replace")
    status_line = header_str.split("\r\n")[0]
    parts = status_line.split(" ", 2)
    if len(parts) >= 2:
        return int(parts[1])
    return None


def _parse_body(data):
    try:
        header_end = data.index(b"\r\n\r\n") + 4
    except ValueError:
        return b""
    header_str = data[:header_end].decode("utf-8", errors="replace")
    body = data[header_end:]
    if "transfer-encoding: chunked" in header_str.lower():
        result = b""
        pos = 0
        while pos < len(body):
            try:
                size_end = body.index(b"\r\n", pos)
            except ValueError:
                break
            size_str = body[pos:size_end].decode("ascii").strip()
            try:
                chunk_size = int(size_str, 16)
            except ValueError:
                break
            if chunk_size == 0:
                break
            data_start = size_end + 2
            data_end = data_start + chunk_size
            if data_end <= len(body):
                result += body[data_start:data_end]
            pos = data_end + 2
        return result
    return body


def _build_request(fixture, addr):
    inp = fixture["input"]
    method = inp["method"]
    headers = dict(inp.get("headers", {}))
    encoding = inp.get("encoding")
    chunk_size = inp.get("chunk_size")
    extra_raw = inp.get("extra_raw_headers")

    if "Host" not in headers and "host" not in {k.lower() for k in headers}:
        headers["Host"] = addr

    body_data = b""
    if inp.get("body") is not None:
        body_data = inp["body"].encode("utf-8")
    elif inp.get("body_hex"):
        body_data = bytes.fromhex(inp["body_hex"])
    elif inp.get("body_partial"):
        body_data = inp["body_partial"].encode("utf-8")

    is_chunked = encoding in ("chunked", "chunked_malformed", "chunked_no_trailer", "chunked_no_terminator")

    if is_chunked:
        headers["Transfer-Encoding"] = "chunked"
    elif "Content-Length" not in headers and "content-length" not in {k.lower() for k in headers}:
        if body_data or inp.get("body") is not None:
            headers["Content-Length"] = str(len(body_data))

    headers["Connection"] = "close"

    header_lines = "".join(f"{k}: {v}\r\n" for k, v in headers.items())
    request = f"{method} /test HTTP/1.1\r\n{header_lines}".encode()

    # Insert extra raw headers (for conflicting Content-Length tests)
    if extra_raw:
        for line in extra_raw.split("\n"):
            line = line.strip()
            if line:
                request += line.encode() + b"\r\n"

    request += b"\r\n"

    if is_chunked:
        if encoding == "chunked_malformed":
            request += b"ZZ\r\n"
            request += body_data
            request += b"\r\n"
            request += b"0\r\n\r\n"
        elif encoding == "chunked_no_trailer":
            cs = chunk_size if chunk_size else len(body_data)
            for i in range(0, len(body_data), cs):
                chunk = body_data[i : i + cs]
                request += f"{len(chunk):x}\r\n".encode() + chunk
            request += b"0\r\n\r\n"
        elif encoding == "chunked_no_terminator":
            cs = chunk_size if chunk_size else len(body_data)
            for i in range(0, len(body_data), cs):
                chunk = body_data[i : i + cs]
                request += f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n"
            # Missing 0-length terminator
        else:
            cs = chunk_size if chunk_size else len(body_data)
            chunked_body = b""
            for i in range(0, len(body_data), cs):
                chunk = body_data[i : i + cs]
                chunked_body += f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n"
            chunked_body += b"0\r\n\r\n"
            request += chunked_body
    else:
        request += body_data

    return request


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformancePolicySelection(unittest.TestCase):
    """Body policy selection: reject, buffer, stream, static."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def _make_server(self, policy, max_body_bytes, handler=None):
        td = tempfile.mkdtemp()
        self._tds.append(td)
        kwargs = {
            "root": td,
            "port": 0,
            "request_body_mode": policy,
        }
        if policy != "reject":
            kwargs["max_request_body_bytes"] = max_body_bytes
        if handler:
            kwargs["handler"] = handler
        s = Server(**kwargs)
        s.start()
        self._servers.append(s)
        _wait_for_tcp(s.addr)
        return s

    def test_policy_selection_from_corpus(self):
        for fixture in _group("body_policy_selection"):
            inp = fixture["input"]
            exp = fixture["expected"]

            if inp["policy"] == "static":
                td = tempfile.mkdtemp()
                self._tds.append(td)
                with open(os.path.join(td, "test.txt"), "w") as f:
                    f.write("ok")
                s = Server(root=td, port=0)
                s.start()
                self._servers.append(s)
                _wait_for_tcp(s.addr)
            else:
                captured = {"called": False}

                def handler(req, _c=captured):
                    _c["called"] = True
                    return Response.text(200, "ok")

                s = self._make_server(
                    inp["policy"],
                    inp["max_body_bytes"],
                    handler=handler,
                )

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured.get("called", False),
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceEmptyBody(unittest.TestCase):
    """Empty body handling."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_empty_body_from_corpus(self):
        for fixture in _group("empty_body"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"data": b""}

            def handler(req, _c=captured):
                if req.has_body:
                    _c["data"] = req.body.read()
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "body_data" in exp:
                self.assertEqual(
                    captured["data"].decode("utf-8", errors="replace"),
                    exp["body_data"],
                    f"{fixture['id']}: body_data",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceFixedLength(unittest.TestCase):
    """Fixed-length body within limit."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_fixed_length_from_corpus(self):
        for fixture in _group("fixed_length_exact"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"data": b""}

            def handler(req, _c=captured):
                if req.has_body:
                    _c["data"] = req.body.read()
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "echo_body" in exp:
                self.assertEqual(
                    captured["data"].decode("utf-8", errors="replace"),
                    exp["echo_body"],
                    f"{fixture['id']}: echo_body",
                )

            if "echo_len" in exp:
                self.assertEqual(
                    len(captured["data"]),
                    exp["echo_len"],
                    f"{fixture['id']}: echo_len",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceOverLimit(unittest.TestCase):
    """Fixed-length body exceeding limit."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_over_limit_from_corpus(self):
        for fixture in _group("fixed_length_over_limit"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                if req.has_body:
                    req.body.read()
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceChunked(unittest.TestCase):
    """Chunked transfer-encoding body."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_chunked_exact_from_corpus(self):
        for fixture in _group("chunked_exact"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            chunks_captured = []

            def handler(req, _cc=chunks_captured):
                if req.has_body:
                    it = req.body.iter_chunks()
                    for chunk in it:
                        _cc.append(chunk)
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "echo_body" in exp:
                self.assertEqual(
                    b"".join(chunks_captured).decode("utf-8", errors="replace"),
                    exp["echo_body"],
                    f"{fixture['id']}: echo_body",
                )

    def test_chunked_over_limit_from_corpus(self):
        for fixture in _group("chunked_over_limit"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                if req.has_body:
                    it = req.body.iter_chunks()
                    for chunk in it:
                        pass
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceOneShot(unittest.TestCase):
    """One-shot consumption enforcement."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_one_shot_from_corpus(self):
        for fixture in _group("one_shot_consumption"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)

            def handler(req):
                if req.has_body:
                    body = req.body
                    # First read always works
                    body.read()
                    # Second read should raise ConsumedError
                    try:
                        body.read()
                        return Response.text(200, "ok")
                    except Exception:
                        return Response.text(200, "consumed")
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_error" in exp:
                body = _parse_body(resp)
                self.assertIn(
                    exp["handler_error"],
                    body.decode("utf-8", errors="replace"),
                    f"{fixture['id']}: handler_error",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceGetWithBody(unittest.TestCase):
    """GET requests with body are rejected."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_get_with_body_from_corpus(self):
        for fixture in _group("get_with_body_rejected"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceConflictingContentLength(unittest.TestCase):
    """Conflicting Content-Length handling."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_conflicting_content_length_from_corpus(self):
        for fixture in _group("conflicting_content_length"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                if req.has_body:
                    req.body.read()
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceChunkedMalformed(unittest.TestCase):
    """Malformed chunked transfer-encoding handling."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_chunked_malformed_from_corpus(self):
        for fixture in _group("chunked_malformed"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                if req.has_body:
                    req.body.read()
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


@unittest.skipUnless(NATIVE_AVAILABLE, "Native module not available")
class TestBodyConformanceChunkedExactLimit(unittest.TestCase):
    """Chunked body at exact limit and one byte over."""

    def setUp(self):
        self._servers = []
        self._tds = []

    def tearDown(self):
        for s in self._servers:
            try:
                s.force_shutdown(2.0)
            except Exception:
                pass
        for td in self._tds:
            shutil.rmtree(td, ignore_errors=True)

    def test_chunked_exact_limit_from_corpus(self):
        for fixture in _group("chunked_exact_limit"):
            inp = fixture["input"]
            exp = fixture["expected"]

            td = tempfile.mkdtemp()
            self._tds.append(td)
            captured = {"called": False}

            def handler(req, _c=captured):
                _c["called"] = True
                if req.has_body:
                    it = req.body.iter_chunks()
                    for chunk in it:
                        pass
                return Response.text(200, "ok")

            s = Server(
                root=td,
                port=0,
                handler=handler,
                request_body_mode=inp["policy"],
                max_request_body_bytes=inp["max_body_bytes"],
            )
            s.start()
            self._servers.append(s)
            _wait_for_tcp(s.addr)

            req_bytes = _build_request(fixture, s.addr)
            resp = _send_raw_request(s.addr, req_bytes)
            status = _parse_status(resp)
            self.assertEqual(status, exp["status"], fixture["id"])

            if "handler_called" in exp:
                self.assertEqual(
                    captured["called"],
                    exp["handler_called"],
                    f"{fixture['id']}: handler_called",
                )


if __name__ == "__main__":
    unittest.main()
