# eggserve

A hardened, Rust-backed static file server, packaged as a Python wheel.

Release wheels support CPython 3.14 only (`>=3.14,<3.15`) and bundle the
platform-native `eggserve` CLI binary.

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
cargo build --release --locked -p eggserve-bin
mkdir -p crates/eggserve-python/python/eggserve/bin
cp target/release/eggserve crates/eggserve-python/python/eggserve/bin/eggserve
chmod +x crates/eggserve-python/python/eggserve/bin/eggserve
cd crates/eggserve-python
maturin build --release --interpreter python3.14 -o dist
```
