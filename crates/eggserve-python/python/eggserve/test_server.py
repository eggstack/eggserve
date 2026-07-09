"""Tests for the eggserve Python API.

Uses stdlib unittest to validate config defaults, argument translation,
public-bind guard, and ServerProcess lifecycle.
"""

import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from eggserve.server import ServeConfig, ServerProcess, StaticPolicy, _config_to_argv


class TestStaticPolicy(unittest.TestCase):
    def test_defaults_are_safe(self):
        policy = StaticPolicy()
        self.assertFalse(policy.directory_listing)
        self.assertFalse(policy.follow_symlinks)
        self.assertFalse(policy.allow_dotfiles)

    def test_frozen(self):
        policy = StaticPolicy()
        with self.assertRaises(AttributeError):
            policy.directory_listing = True  # type: ignore[misc]

    def test_explicit_overrides(self):
        policy = StaticPolicy(
            directory_listing=True, follow_symlinks=True, allow_dotfiles=True
        )
        self.assertTrue(policy.directory_listing)
        self.assertTrue(policy.follow_symlinks)
        self.assertTrue(policy.allow_dotfiles)


class TestServeConfig(unittest.TestCase):
    def test_defaults_match_documented(self):
        config = ServeConfig()
        self.assertEqual(config.directory, ".")
        self.assertEqual(config.bind, "127.0.0.1")
        self.assertEqual(config.port, 8000)
        self.assertFalse(config.public)
        self.assertEqual(config.log_format, "text")
        self.assertIsInstance(config.policy, StaticPolicy)
        self.assertFalse(config.policy.directory_listing)
        self.assertFalse(config.policy.follow_symlinks)
        self.assertFalse(config.policy.allow_dotfiles)

    def test_frozen(self):
        config = ServeConfig()
        with self.assertRaises(AttributeError):
            config.port = 9000  # type: ignore[misc]

    def test_path_directory(self):
        config = ServeConfig(directory=Path("/tmp/public"))
        self.assertEqual(config.directory, Path("/tmp/public"))


class TestConfigToArgv(unittest.TestCase):
    def test_minimal_config(self):
        argv = _config_to_argv(ServeConfig())
        self.assertIn("--directory", argv)
        self.assertIn(".", argv)
        self.assertIn("--bind", argv)
        self.assertIn("127.0.0.1", argv)
        self.assertIn("--port", argv)
        self.assertIn("8000", argv)
        self.assertNotIn("--public", argv)
        self.assertNotIn("--directory-listing", argv)
        self.assertNotIn("--follow-symlinks", argv)
        self.assertNotIn("--allow-dotfiles", argv)

    def test_public_flag(self):
        argv = _config_to_argv(ServeConfig(public=True))
        self.assertIn("--public", argv)

    def test_directory_listing(self):
        argv = _config_to_argv(
            ServeConfig(policy=StaticPolicy(directory_listing=True))
        )
        self.assertIn("--directory-listing", argv)

    def test_follow_symlinks(self):
        argv = _config_to_argv(
            ServeConfig(policy=StaticPolicy(follow_symlinks=True))
        )
        self.assertIn("--follow-symlinks", argv)

    def test_allow_dotfiles(self):
        argv = _config_to_argv(
            ServeConfig(policy=StaticPolicy(allow_dotfiles=True))
        )
        self.assertIn("--allow-dotfiles", argv)

    def test_all_flags(self):
        config = ServeConfig(
            directory="/var/www",
            bind="0.0.0.0",
            port=3000,
            public=True,
            policy=StaticPolicy(
                directory_listing=True, follow_symlinks=True, allow_dotfiles=True
            ),
            log_format="json",
        )
        argv = _config_to_argv(config)
        self.assertIn("--directory", argv)
        idx = argv.index("--directory")
        self.assertEqual(argv[idx + 1], "/var/www")
        self.assertIn("--bind", argv)
        idx = argv.index("--bind")
        self.assertEqual(argv[idx + 1], "0.0.0.0")
        self.assertIn("--port", argv)
        idx = argv.index("--port")
        self.assertEqual(argv[idx + 1], "3000")
        self.assertIn("--public", argv)
        self.assertIn("--directory-listing", argv)
        self.assertIn("--follow-symlinks", argv)
        self.assertIn("--allow-dotfiles", argv)
        self.assertIn("--log-format", argv)
        idx = argv.index("--log-format")
        self.assertEqual(argv[idx + 1], "json")

    def test_log_format_text_omitted(self):
        argv = _config_to_argv(ServeConfig(log_format="text"))
        self.assertNotIn("--log-format", argv)

    def test_log_format_none(self):
        argv = _config_to_argv(ServeConfig(log_format="none"))
        self.assertIn("--log-format", argv)
        idx = argv.index("--log-format")
        self.assertEqual(argv[idx + 1], "none")


class TestConfigValidation(unittest.TestCase):
    def test_invalid_log_format_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(log_format="xml")
        self.assertIn("log_format", str(ctx.exception))

    def test_port_zero_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(port=0)
        self.assertIn("port", str(ctx.exception))

    def test_port_above_65535_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(port=70000)
        self.assertIn("port", str(ctx.exception))

    def test_negative_port_raises(self):
        with self.assertRaises(ValueError):
            ServeConfig(port=-1)

    def test_non_int_port_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(port="8000")
        self.assertIn("port", str(ctx.exception))

    def test_bool_port_raises(self):
        with self.assertRaises(ValueError):
            ServeConfig(port=True)

    def test_public_ipv4_without_public_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="0.0.0.0", public=False)
        self.assertIn("public=True", str(ctx.exception))

    def test_public_ipv6_without_public_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="::", public=False)
        self.assertIn("public=True", str(ctx.exception))

    def test_valid_public_bind_with_public_true_allowed(self):
        config = ServeConfig(bind="0.0.0.0", public=True)
        self.assertEqual(config.bind, "0.0.0.0")
        self.assertTrue(config.public)

    def test_valid_ipv6_public_bind_allowed(self):
        config = ServeConfig(bind="::", public=True, port=9000)
        self.assertEqual(config.bind, "::")
        self.assertEqual(config.port, 9000)

    def test_valid_log_formats_accepted(self):
        for fmt in ("text", "json", "none"):
            config = ServeConfig(log_format=fmt)
            self.assertEqual(config.log_format, fmt)

    def test_boundary_ports_accepted(self):
        ServeConfig(port=1)
        ServeConfig(port=65535)


class TestLegacyPublicBindGuard(unittest.TestCase):
    """Pre-validation construction succeeded, so this exercises the
    documented behavior that public-bind misconfiguration is caught at
    ServeConfig construction, not at ServerProcess.start()."""

    def test_construction_raises_before_start(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="0.0.0.0", public=False)
        self.assertIn("public=True", str(ctx.exception))

    def test_unspecified_ipv6_raises_at_construction(self):
        with self.assertRaises(ValueError):
            ServeConfig(bind="::", public=False)

    def test_bind_with_mismatched_port_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="0.0.0.0:9000", public=True)
        self.assertIn("port", str(ctx.exception))

    def test_bind_with_mismatched_ipv6_port_raises(self):
        with self.assertRaises(ValueError) as ctx:
            ServeConfig(bind="[::]:9000", public=True)
        self.assertIn("port", str(ctx.exception))

    def test_bind_host_port_form_without_public_raises(self):
        with self.assertRaises(ValueError):
            ServeConfig(bind="0.0.0.0:9000", public=False)

    def test_bind_ipv6_host_port_form_without_public_raises(self):
        with self.assertRaises(ValueError):
            ServeConfig(bind="[::]:9000", public=False)

    def test_bind_host_port_form_with_matching_port_accepted(self):
        config = ServeConfig(bind="0.0.0.0:9000", public=True, port=9000)
        self.assertEqual(config.bind, "0.0.0.0:9000")
        self.assertEqual(config.port, 9000)

    def test_bind_ipv6_host_port_form_with_matching_port_accepted(self):
        config = ServeConfig(bind="[::]:9000", public=True, port=9000)
        self.assertEqual(config.bind, "[::]:9000")
        self.assertEqual(config.port, 9000)

    def test_bind_invalid_address_raises(self):
        with self.assertRaises(ValueError):
            ServeConfig(bind="not-an-address")


class TestServerProcess(unittest.TestCase):
    def test_not_running_before_start(self):
        proc = ServerProcess(ServeConfig())
        self.assertFalse(proc.is_running)
        self.assertIsNone(proc.pid)

    def test_already_running_raises(self):
        proc = ServerProcess(ServeConfig())
        proc._process = MagicMock()
        proc._process.poll.return_value = None
        with self.assertRaises(RuntimeError) as ctx:
            proc.start()
        self.assertIn("already running", str(ctx.exception))

    def test_stop_without_start_is_noop(self):
        proc = ServerProcess(ServeConfig())
        proc.stop()

    @patch("eggserve.server._find_binary")
    @patch("eggserve.server.subprocess.Popen")
    def test_start_spawns_binary(self, mock_popen, mock_find_binary):
        mock_find_binary.return_value = "/usr/bin/eggserve"
        mock_process = MagicMock()
        mock_process.poll.return_value = None
        mock_popen.return_value = mock_process

        config = ServeConfig(port=9000, log_format="none")
        proc = ServerProcess(config)
        proc.start()

        mock_find_binary.assert_called_once()
        mock_popen.assert_called_once()
        call_args = mock_popen.call_args
        argv = call_args[0][0]
        self.assertEqual(argv[0], "/usr/bin/eggserve")
        self.assertIn("--port", argv)
        self.assertIn("9000", argv)
        self.assertTrue(proc.is_running)

    @patch("eggserve.server._find_binary")
    @patch("eggserve.server.subprocess.Popen")
    def test_stop_terminates_process(self, mock_popen, mock_find_binary):
        mock_find_binary.return_value = "/usr/bin/eggserve"
        mock_process = MagicMock()
        mock_process.poll.return_value = None
        mock_process.wait.return_value = 0
        mock_popen.return_value = mock_process

        proc = ServerProcess(ServeConfig())
        proc.start()
        proc.stop()

        mock_process.terminate.assert_called_once()
        mock_process.wait.assert_called_once()
        self.assertFalse(proc.is_running)

    @patch("eggserve.server._find_binary")
    @patch("eggserve.server.subprocess.Popen")
    def test_wait_returns_exit_code(self, mock_popen, mock_find_binary):
        mock_find_binary.return_value = "/usr/bin/eggserve"
        mock_process = MagicMock()
        mock_process.wait.return_value = 0
        mock_popen.return_value = mock_process

        proc = ServerProcess(ServeConfig())
        proc.start()
        code = proc.wait()

        self.assertEqual(code, 0)

    def test_wait_without_start_raises(self):
        proc = ServerProcess(ServeConfig())
        with self.assertRaises(RuntimeError):
            proc.wait()

    @patch("eggserve.server._find_binary")
    def test_binary_not_found_raises(self, mock_find_binary):
        mock_find_binary.side_effect = FileNotFoundError("not found")
        proc = ServerProcess(ServeConfig())
        with self.assertRaises(FileNotFoundError):
            proc.start()


if __name__ == "__main__":
    unittest.main()
