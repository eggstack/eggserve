from pathlib import Path

class StaticPolicy:
    directory_listing: bool
    follow_symlinks: bool
    allow_dotfiles: bool
    def __init__(
        self,
        directory_listing: bool = ...,
        follow_symlinks: bool = ...,
        allow_dotfiles: bool = ...,
    ) -> None: ...

class ServeConfig:
    directory: str | Path
    bind: str
    port: int
    public: bool
    policy: StaticPolicy
    log_format: str
    def __init__(
        self,
        directory: str | Path = ...,
        bind: str = ...,
        port: int = ...,
        public: bool = ...,
        policy: StaticPolicy = ...,
        log_format: str = ...,
    ) -> None: ...

class ServerProcess:
    def __init__(self, config: ServeConfig) -> None: ...
    def start(self) -> None: ...
    def stop(self, timeout: float | None = ...) -> None: ...
    def wait(self) -> int: ...
    @property
    def is_running(self) -> bool: ...
    @property
    def pid(self) -> int | None: ...

def serve_directory(
    directory: str | Path = ...,
    *,
    bind: str = ...,
    port: int = ...,
    public: bool = ...,
    policy: StaticPolicy | None = ...,
    log_format: str = ...,
) -> None: ...
def _parse_bind(bind: str) -> tuple[str, int | None]: ...
def _config_to_argv(config: ServeConfig) -> list[str]: ...
