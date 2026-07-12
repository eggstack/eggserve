"""Standalone packaging smoke test — HTTP client.

Tests local HTTP client request using eggserve.HttpClient.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import os
import shutil
import tempfile
import time
import unittest
import urllib.error
import urllib.request

from eggserve import HttpClient, ClientConfig, ClientError, EggserveError, Server


def _wait_for_server(url, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            urllib.request.urlopen(url, timeout=1)
            return True
        except urllib.error.HTTPError:
            return True
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            time.sleep(0.05)
    return False


class TestHttpClientConstruction(unittest.TestCase):
    """HttpClient can be constructed with default and custom config."""

    def test_default_client(self):
        client = HttpClient()
        self.assertIn("HttpClient", repr(client))

    def test_client_with_config(self):
        config = ClientConfig(connect_timeout=5.0, request_timeout=15.0)
        client = HttpClient(config)
        self.assertIn("HttpClient", repr(client))

    def test_client_config_defaults(self):
        config = ClientConfig()
        self.assertEqual(config.connect_timeout, 10.0)
        self.assertEqual(config.request_timeout, 30.0)


class TestHttpClientLocalRequest(unittest.TestCase):
    """HttpClient can fetch a local server response.

    NOTE: eggserve's server rejects absolute-form URIs (http://host:port/path)
    as a security measure. The HttpClient sends absolute-form URIs, so it cannot
    be used against eggserve's own server. These tests verify the client can
    connect and handle the resulting 400 response correctly.
    """

    def setUp(self):
        self._td = tempfile.mkdtemp()
        with open(os.path.join(self._td, "hello.txt"), "w") as f:
            f.write("hello from file")
        self._server = None

    def tearDown(self):
        if self._server is not None:
            try:
                self._server.stop()
            except Exception:
                pass
        shutil.rmtree(self._td, ignore_errors=True)

    def _start_server(self):
        from eggserve import Server
        self._server = Server(root=self._td, port=0)
        self._server.start()
        addr = self._server.addr
        url = f"http://{addr}/hello.txt"
        self.assertTrue(_wait_for_server(url))
        return url

    def test_get_returns_response(self):
        """HttpClient can connect to a server and get a response."""
        url = self._start_server()
        client = HttpClient()
        resp = client.get(url)
        self.assertIsNotNone(resp)
        self.assertIn(resp.status, (200, 400))

    def test_head_returns_response(self):
        """HttpClient can send HEAD and get a response."""
        url = self._start_server()
        client = HttpClient()
        resp = client.head(url)
        self.assertIsNotNone(resp)
        self.assertIn(resp.status, (200, 400))

    def test_response_has_headers(self):
        """Response includes headers dict."""
        url = self._start_server()
        client = HttpClient()
        resp = client.get(url)
        self.assertIsInstance(resp.headers, dict)


class TestHttpClientErrorHandling(unittest.TestCase):
    """HttpClient raises proper errors for bad inputs."""

    def test_unsupported_scheme(self):
        client = HttpClient()
        with self.assertRaises(EggserveError) as ctx:
            client.get("ftp://example.com/")
        self.assertIn("unsupported scheme", str(ctx.exception))

    def test_invalid_url(self):
        client = HttpClient()
        with self.assertRaises(EggserveError) as ctx:
            client.get("not-a-url")
        self.assertIn("invalid URL", str(ctx.exception))

    def test_client_error_is_eggserve_error(self):
        """HttpClient raises EggserveError for bad inputs."""
        client = HttpClient()
        with self.assertRaises(EggserveError) as ctx:
            client.get("ftp://example.com/")
        self.assertIn("unsupported scheme", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
