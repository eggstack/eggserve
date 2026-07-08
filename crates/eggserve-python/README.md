# eggserve

A hardened, Rust-backed static file server, packaged as a Python wheel.

## Installation

```bash
pip install eggserve
```

## Usage

```bash
eggserve
eggserve 8000
eggserve --directory public
python -m eggserve --directory public 8000
```

## Safe defaults

- Binds to 127.0.0.1 (loopback only)
- No symlink following
- No dotfile serving
- No directory listing
- GET and HEAD only
- Request bodies rejected

## Building from source

```bash
pip install maturin
cd crates/eggserve-python
maturin build --release
```
