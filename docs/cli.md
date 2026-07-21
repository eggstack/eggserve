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
| `--addr HOST:PORT` | Bind address (sets both host and port; blocks positional port override) | `127.0.0.1:8000` |
| `--bind HOST[:PORT]` | Bind host or host:port (positional port can override the port portion) | `127.0.0.1:8000` |
| `--port PORT` | Port to listen on (blocks positional port override) | `8000` |
| `PORT` | Positional port argument (overrides `--bind` port when neither `--port` nor `--addr` is given) | `8000` |
| `--public` | Bind to all interfaces (required for `0.0.0.0` or `::` binds) | off |

Port resolution: `--port`/`--addr` take highest precedence and block positional override. The `PORT` positional overrides `--bind` when neither `--port` nor `--addr` is given. Default is `127.0.0.1:8000`.

Binding to `0.0.0.0` or `::` without `--public` is rejected with an error.

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
| `--max-listing-entries N` | Maximum directory listing entries | `4096` |
| `--max-listing-response-bytes N` | Maximum directory listing response body bytes | `1048576` (1 MiB) |
| `--max-listing-filename-bytes N` | Maximum single filename bytes in listing | `255` |
| `--listing-enumeration-timeout SECS` | Directory enumeration timeout | `30` |

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
