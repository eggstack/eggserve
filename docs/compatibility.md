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
| Symlinks | platform behavior | denied | `--follow-symlinks` |
| Methods | basic GET/HEAD | GET/HEAD | none initially |
| CGI | separate module | unsupported | unsupported |
| Dotfiles | served | denied | (not yet available) |
| `python -m` invocation | `python -m http.server` | `python -m eggserve` | (deferred to plan 005) |

## Invocation shape

```
python -m http.server [PORT] [DIRECTORY]           # Python stdlib
eggserve [DIRECTORY] [--bind HOST:PORT]            # eggserve
python -m eggserve [DIRECTORY] [--bind HOST:PORT]  # eggserve via Python (deferred)
```
