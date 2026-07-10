"""Tests for HTTP client native primitives."""

import unittest
from eggserve._native import (
    ClientConfig,
    ClientError,
    EggserveError,
    HttpClient,
    Method,
)


class TestClientConfig(unittest.TestCase):
    def test_default_config(self):
        config = ClientConfig()
        self.assertEqual(config.connect_timeout, 10.0)
        self.assertEqual(config.request_timeout, 30.0)
        self.assertEqual(config.max_response_body_bytes, 10_485_760)
        self.assertTrue(config.verify_tls)

    def test_custom_config(self):
        config = ClientConfig(
            connect_timeout=5.0,
            request_timeout=15.0,
            max_response_body_bytes=1024,
            verify_tls=False,
        )
        self.assertEqual(config.connect_timeout, 5.0)
        self.assertEqual(config.request_timeout, 15.0)
        self.assertEqual(config.max_response_body_bytes, 1024)
        self.assertFalse(config.verify_tls)

    def test_config_repr(self):
        config = ClientConfig()
        r = repr(config)
        self.assertIn("ClientConfig", r)
        self.assertIn("connect_timeout", r)


class TestClientConfigValidation(unittest.TestCase):
    def test_negative_connect_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(connect_timeout=-1.0)
        self.assertIn("connect_timeout", str(ctx.exception))

    def test_negative_request_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(request_timeout=-1.0)
        self.assertIn("request_timeout", str(ctx.exception))

    def test_nan_connect_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(connect_timeout=float("nan"))
        self.assertIn("connect_timeout", str(ctx.exception))

    def test_nan_request_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(request_timeout=float("nan"))
        self.assertIn("request_timeout", str(ctx.exception))

    def test_infinite_connect_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(connect_timeout=float("inf"))
        self.assertIn("connect_timeout", str(ctx.exception))

    def test_infinite_request_timeout(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(request_timeout=float("inf"))
        self.assertIn("request_timeout", str(ctx.exception))

    def test_zero_max_response_body_bytes_rejected(self):
        with self.assertRaises(ValueError) as ctx:
            ClientConfig(max_response_body_bytes=0)
        self.assertIn("max_response_body_bytes", str(ctx.exception))

    def test_valid_boundary_values(self):
        config = ClientConfig(
            connect_timeout=0.0,
            request_timeout=0.0,
            max_response_body_bytes=1,
        )
        self.assertEqual(config.connect_timeout, 0.0)
        self.assertEqual(config.request_timeout, 0.0)
        self.assertEqual(config.max_response_body_bytes, 1)


class TestMethod(unittest.TestCase):
    def test_method_variants_exist(self):
        self.assertIsNotNone(Method.Get)
        self.assertIsNotNone(Method.Head)
        self.assertIsNotNone(Method.Post)
        self.assertIsNotNone(Method.Put)
        self.assertIsNotNone(Method.Delete)
        self.assertIsNotNone(Method.Patch)

    def test_method_repr(self):
        self.assertIn("Get", repr(Method.Get))


class TestHttpClient(unittest.TestCase):
    def test_create_default(self):
        client = HttpClient()
        self.assertIn("HttpClient", repr(client))

    def test_create_with_config(self):
        config = ClientConfig(connect_timeout=5.0)
        client = HttpClient(config)
        self.assertIn("HttpClient", repr(client))

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


class TestClientError(unittest.TestCase):
    def test_error_is_eggserve_error(self):
        with self.assertRaises(EggserveError) as ctx:
            HttpClient().get("ftp://example.com/")
        self.assertIn("unsupported scheme", str(ctx.exception))

    def test_error_message_contains_details(self):
        with self.assertRaises(EggserveError) as ctx:
            HttpClient().get("ftp://example.com/")
        self.assertIn("unsupported scheme", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
