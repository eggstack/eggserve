# CLI Reference

eggserve ships as a CLI binary and a Python-packaged tool. Both share the same argument interface.

## Usage

```
eggserve [OPTIONS] [PORT] [--directory DIR]
```

## Options

### Server

| Flag | Description | Default |
|------|-------------|---------|
| `--directory DIR` | Root directory to serve | `.` (current directory) |
| `--addr HOST:PORT` | Bind address (sets both host and port) | `127.0.0.1:8000` |
| `--bind HOST[:PORT]` | Bind host or host:port (alias for `--addr`) | `127.0.0.1:8000` |
| `--port PORT` | Port to listen on (overrides `--addr` port) | `8000` |
| `PORT` | Positional port argument (overrides all port sources) | `8000` |
| `--public` | Bind to all interfaces (required for non-loopback binds) | off |

Port resolution order: `PORT` positional > `--port` > `--addr`/`--bind` > default `8000`.

Binding to `0.0.0.0` or a non-loopback address without `--public` is rejected with an error.

### Security policies

| Flag | Description | Default |
|------|-------------|---------|
| `--directory-listing` | Enable HTML directory listing | disabled |
| `--follow-symlinks` | Follow symlinks outside root | denied |
| `--allow-dotfiles` | Serve dotfiles (e.g. `.env`, `.git`) | denied |

### Resource limits

| Flag | Description | Default |
|------|-------------|---------|
| `--max-connections N` | Maximum concurrent connections | `64` |
| `--max-file-streams N` | Maximum concurrent file streams | `32` |

### Timeouts

| Flag | Description | Default |
|------|-------------|---------|
| `--header-timeout SECS` | Header read timeout (seconds) | `10` |
| `--write-timeout SECS` | Response write timeout (seconds) | `60` |

### Output

| Flag | Description | Default |
|------|-------------|---------|
| `--log-format FORMAT` | Log format: `text`, `json`, or `none` | `text` |
| `--quiet` | Suppress startup banner | off |
| `-h`, `--help` | Print help and exit | |
| `-V`, `--version` | Print version and exit | |

## Examples

```sh
# Serve current directory on default port
eggserve

# Serve a specific directory on port 3000
eggserve --directory ./public 3000

# Bind to all interfaces (public server)
eggserve --public --addr 0.0.0.0:8080

# Enable directory listing and dotfiles
eggserve --directory-listing --allow-dotfiles

# JSON logging, quiet startup
eggserve --log-format json --quiet

# Custom resource limits
eggserve --max-connections 128 --max-file-streams 64
```

## Python launcher

When installed via pip, the same CLI is available as:

```sh
python -m eggserve [OPTIONS] [PORT]
pipx run eggserve [OPTIONS] [PORT]
```

Arguments are forwarded directly to the bundled Rust binary.
