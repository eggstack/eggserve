# CLI Reference

eggserve ships as a CLI binary and a Python-packaged tool. Both share the same argument interface.

## Usage

```
eggserve [OPTIONS] [PORT] [DIRECTORY]
```

## Options

### Server

| Flag | Description | Default |
|------|-------------|---------|
| `--directory DIR` | Root directory to serve | `.` (current directory) |
| `--addr HOST:PORT` | Bind address (cannot be combined with `--bind`) | `127.0.0.1:8000` |
| `--bind HOST[:PORT]` | Bind host or host:port; an explicit port is retained | `127.0.0.1:8000` |
| `--port PORT` | Port to listen on (blocks positional port override) | `8000` |
| `PORT` | Positional port argument; a valid numeric token fills the unoccupied port slot | `8000` |
| `DIRECTORY` | First positional token not consumed as PORT; once PORT is occupied, it is used verbatim, including numeric names | `.` (current directory) |
| `--public` | Bind to all interfaces (required for `0.0.0.0` or `::` binds) | off |

`--bind` accepts IPv4/IPv6 literals and hostnames. Hostname resolution happens
once during argument validation; the resolved address is then used for the
listener. A hostname that resolves to a wildcard address still requires
`--public`.

Note: `--bind HOST` (without a port) leaves the positional port slot
available. A subsequent numeric positional argument will be parsed as the
port. For example, `eggserve --bind 127.0.0.1 8000 mydir` binds port 8000
and serves `mydir`. If the numeric argument is intended as a directory name
rather than a port, use `--port` to explicitly set the port first (e.g.
`eggserve --bind 127.0.0.1 --port 9000 1234`).

Positional arguments use two logical slots: `PORT`, then `DIRECTORY`. `--port`,
`--addr`, and an explicit port in `--bind` occupy the port slot. A host-only
`--bind` leaves that slot available for a positional numeric token. Once the
port slot is occupied, the next positional token is the directory verbatim,
including a token made only of decimal digits. `--directory` occupies the
directory slot, so a later numeric positional token can still provide PORT.
After both slots are occupied, another positional argument is rejected. A
single valid numeric positional token remains PORT for compatibility. `--addr`
sets both host and port and cannot be combined with `--bind`. Default is
`127.0.0.1:8000`.

`--header-timeout` must not exceed `--connection-total-timeout`.

Binding to `0.0.0.0` or `::` without `--public` is rejected with an error.

### Security policies

| Flag | Description | Default |
|------|-------------|---------|
| `--directory-listing` | Enable HTML directory listing | disabled |
| `--follow-symlinks` | Follow symlinks outside root | denied |
| `--allow-dotfiles` | Serve dotfiles (e.g. `.env`, `.git`) | denied |

### Static response metadata

| Flag | Description | Default |
|------|-------------|---------|
| `--content-type TYPE` | Content type used when a file suffix has no detected MIME type | `application/octet-stream` |
| `-H NAME VALUE`, `--header NAME VALUE` | Add a repeatable safe response header to final `200` static responses | none |

Header values preserve command-line order. Runtime-owned metadata and
hop-by-hop headers are rejected rather than silently overridden.

### Resource limits

| Flag | Description | Default |
|------|-------------|---------|
| `--max-connections N` | Maximum concurrent connections | `64` |
| `--max-file-streams N` | Maximum concurrent file streams | `32` |

### Timeouts

| Flag | Description | Default |
|------|-------------|---------|
| `--header-timeout SECS` | Header read timeout (seconds) | `10` |
| `--connection-total-timeout SECS` | Total connection lifetime timeout (seconds) | `60` |
| `--handler-timeout SECS` | Handler invocation timeout (seconds) | `30` |
| `--body-read-timeout SECS` | Request body read timeout (seconds) | `30` |

### Output

| Flag | Description | Default |
|------|-------------|---------|
| `--log-format FORMAT` | Log format: `text` (default), `json` (JSON Lines), `none` — `json` outputs valid JSON Lines to stderr; `none` suppresses all operational logs | `text` |
| `--quiet` | Suppress routine informational output (warn/error only) | off |
| `-h`, `--help` | Print help and exit | |
| `-V`, `--version` | Print version and exit | |

## Examples

```sh
# Serve current directory on default port
eggserve

# Serve a specific directory on port 3000
eggserve --directory ./public 3000

# Serve a directory whose name is numeric after an explicit port source
eggserve --port 9000 1234

# Bind to all interfaces (public server)
eggserve --public --addr 0.0.0.0:8080

# Enable directory listing and dotfiles
eggserve --directory-listing --allow-dotfiles

# JSON logging, suppress info events
eggserve --log-format json --quiet

# Custom resource limits
eggserve --max-connections 128 --max-file-streams 64
```

## Python launcher

When installed via pip, the same CLI is available as:

```sh
python -m eggserve [OPTIONS] [PORT] [DIRECTORY]
pipx run eggserve [OPTIONS] [PORT] [DIRECTORY]
```

Arguments are forwarded directly to the extension-linked Rust CLI entry point
inside the installed wheel.
