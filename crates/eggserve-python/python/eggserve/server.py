"""Python API for eggserve.

Provides a programmatic interface to the eggserve static file server.
The Rust binary is the source of truth for all serving logic. This module
translates Python config objects to CLI arguments and manages the binary
process lifecycle.

This is NOT an ASGI/WSGI server, a web framework, or a request callback
system. It is a hardened static-serving primitive.
"""

from __future__ import annotations

import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, Optional

from eggserve._bin import _find_binary


__all__ = [
    "StaticPolicy",
    "ServeConfig",
    "ServerProcess",
    "serve_directory",
]


@dataclass(frozen=True)
class StaticPolicy:
    """Filesystem access policy for the server.

    All defaults are safe. Unsafe behaviors require explicit opt-in.
    """

    directory_listing: bool = False
    follow_symlinks: bool = False
    allow_dotfiles: bool = False


_VALID_LOG_FORMATS = frozenset({"text", "json", "none"})
_PUBLIC_BIND_VALUES = frozenset({"0.0.0.0", "::"})


@dataclass(frozen=True)
class ServeConfig:
    """Configuration for the eggserve static file server.

    Defaults match the CLI and Rust core safe-by-default behavior:
    loopback bind, no directory listing, no symlinks, no dotfiles.

    Validation runs in ``__post_init__``: an invalid port, ``log_format``,
    or public-bind combination raises ``ValueError`` before any subprocess
    is spawned. The Rust CLI performs the same checks independently as
    defense in depth.
    """

    directory: str | Path = "."
    bind: str = "127.0.0.1"
    port: int = 8000
    public: bool = False
    policy: StaticPolicy = field(default_factory=StaticPolicy)
    log_format: Literal["text", "json", "none"] = "text"

    def __post_init__(self) -> None:
        if not isinstance(self.port, int) or isinstance(self.port, bool):
            raise ValueError(
                f"port must be an int, got {type(self.port).__name__}: {self.port!r}"
            )
        if not (1 <= self.port <= 65535):
            raise ValueError(
                f"port must be between 1 and 65535, got {self.port}"
            )
        if self.log_format not in _VALID_LOG_FORMATS:
            raise ValueError(
                f"log_format must be one of {sorted(_VALID_LOG_FORMATS)}, "
                f"got {self.log_format!r}"
            )
        if not self.public and self.bind in _PUBLIC_BIND_VALUES:
            raise ValueError(
                f"binding to {self.bind} requires public=True "
                "to acknowledge public exposure intent"
            )


def _config_to_argv(config: ServeConfig) -> list[str]:
    """Translate a ServeConfig into CLI arguments for the eggserve binary."""
    argv: list[str] = []

    argv.extend(["--directory", str(config.directory)])
    argv.extend(["--bind", config.bind])
    argv.extend(["--port", str(config.port)])

    if config.public:
        argv.append("--public")

    if config.policy.directory_listing:
        argv.append("--directory-listing")
    if config.policy.follow_symlinks:
        argv.append("--follow-symlinks")
    if config.policy.allow_dotfiles:
        argv.append("--allow-dotfiles")

    if config.log_format != "text":
        argv.extend(["--log-format", config.log_format])

    return argv


def serve_directory(
    directory: str | Path = ".",
    *,
    bind: str = "127.0.0.1",
    port: int = 8000,
    public: bool = False,
    policy: Optional[StaticPolicy] = None,
    log_format: Literal["text", "json", "none"] = "text",
) -> None:
    """Start a blocking static file server.

    Runs until interrupted (KeyboardInterrupt) or the process exits.
    This is a programmatic equivalent of ``eggserve`` on the command line.

    Args:
        directory: Root directory to serve (default: current directory).
        bind: Bind address (default: 127.0.0.1).
        port: Listen port (default: 8000).
        public: Acknowledge public exposure intent (required for 0.0.0.0).
        policy: Filesystem access policy (safe defaults if omitted).
        log_format: Log output format: "text", "json", or "none".

    Raises:
        ValueError: If configuration is invalid (port, log_format, or
            public-bind combination).
        FileNotFoundError: If the eggserve binary is not found.
    """
    config = ServeConfig(
        directory=directory,
        bind=bind,
        port=port,
        public=public,
        policy=policy or StaticPolicy(),
        log_format=log_format,
    )
    proc = ServerProcess(config)
    proc.start()
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.stop()


class ServerProcess:
    """Manage an eggserve subprocess.

    Wraps the eggserve binary for use in tests and simple embedding.
    This is a subprocess lifecycle manager, not a Python server object.
    """

    def __init__(self, config: ServeConfig) -> None:
        self._config = config
        self._process: Optional[subprocess.Popen] = None

    def start(self) -> None:
        """Start the server subprocess.

        Raises:
            FileNotFoundError: If the eggserve binary is not found.
            RuntimeError: If the server is already running.
        """
        if self._process is not None:
            raise RuntimeError("server is already running")

        config = self._config

        binary = _find_binary()
        argv = [binary] + _config_to_argv(config)

        self._process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE if config.log_format == "none" else None,
            stderr=subprocess.PIPE if config.log_format == "none" else None,
        )

    def stop(self, timeout: float | None = None) -> None:
        """Stop the server subprocess.

        Args:
            timeout: Seconds to wait for graceful shutdown before killing.
        """
        if self._process is None:
            return

        self._process.terminate()
        try:
            self._process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait()
        self._process = None

    def wait(self) -> int:
        """Wait for the server to exit. Returns the exit code."""
        if self._process is None:
            raise RuntimeError("server is not running")
        returncode = self._process.wait()
        self._process = None
        return returncode

    @property
    def is_running(self) -> bool:
        """Check if the server subprocess is still running."""
        if self._process is None:
            return False
        return self._process.poll() is None

    @property
    def pid(self) -> Optional[int]:
        """The PID of the server subprocess, or None if not started."""
        if self._process is None:
            return None
        return self._process.pid
