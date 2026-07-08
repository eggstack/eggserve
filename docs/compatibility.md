# Compatibility with `python -m http.server`

## What compatibility means

eggserve aims for **practical** compatibility with `python -m http.server`, not behavioral identity:

- **Similar command shape** — `eggserve [DIR]` works like `python -m http.server [DIR]`
- **Similar simple local serving workflow** — serve a directory with one command
- **Similar directory argument semantics where safe** — a positional argument selects the root
- **Not identical filesystem behavior** — eggserve is stricter by default
- **Not identical directory listing defaults** — directory listing is off by default
- **Not identical public bind behavior** — eggserve binds to loopback, not all interfaces
- **Not preserving unsafe traversal/symlink/dotfile behavior** — these are denied unless explicitly opted in

The goal is that a user familiar with `python -m http.server` can switch to `eggserve` with minimal friction, while gaining secure defaults.

## Compatibility matrix

| Feature | `python -m http.server` | eggserve default | eggserve opt-in |
|---------|------------------------|------------------|-----------------|
| Bind default | varies by invocation | loopback | `--public` |
| Directory listing | enabled | disabled | `--directory-listing` |
| Symlinks | platform behavior | denied (final and intermediate) | `--follow-symlinks` (final canonical target must remain inside root) |
| Methods | basic GET/HEAD | GET/HEAD | none initially |
| CGI | separate module | unsupported | unsupported |
| Dotfiles | served | denied | `--allow-dotfiles` |
| Percent encoding | single-pass decode | conservative single-pass decode | — |
| `python -m` invocation | `python -m http.server` | `python -m eggserve` | (deferred to plan 005) |

### Percent encoding behavior

eggserve performs single-pass percent decoding. Double-encoded paths (`%252e%252e`) decode to literal filenames (`%2e%2e`), not to traversal sequences. This is more conservative than `python -m http.server`, which may follow double-encoded paths differently. In practice, this means double-encoded traversal attempts will resolve to 404 (file not found) rather than escaping the root.

## Invocation shape

```
python -m http.server [PORT] [DIRECTORY]           # Python stdlib
eggserve [DIRECTORY] [--bind HOST:PORT]            # eggserve
python -m eggserve [DIRECTORY] [--bind HOST:PORT]  # eggserve via Python (deferred)
```

## Current limitations vs python -m http.server

eggserve currently does not support:

- **Range requests** — full-file streaming only
- **HTTP/2** — HTTP/1.1 only
- **CGI** — not supported (deferred as a non-goal)
- **PUT/POST/DELETE** — read-only by design
- **IPv6** — IPv4 only in alpha (127.0.0.1, not ::1)
- **Multiple directory roots** — single root directory only

These are documented limitations, not bugs. See [non-goals.md](non-goals.md) for the full list.
