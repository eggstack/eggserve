#!/usr/bin/env python3
"""Enforce the Plan 211 Cargo dependency topology.

This is intentionally a small metadata check rather than a line-count or
source-layout rule. Cargo's resolved direct package graph is the contract:
canonical primitives are a leaf, the generic server does not pull static
serving, and static serving consumes the two lower layers.
"""

from __future__ import annotations

import json
import subprocess
import sys


def main() -> int:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            text=True,
        )
    )
    packages = {package["name"]: package for package in metadata["packages"]}

    required = {"eggserve-primitives", "eggserve-server", "eggserve-static"}
    missing = required - packages.keys()
    if missing:
        print(f"missing Plan 211 packages: {', '.join(sorted(missing))}", file=sys.stderr)
        return 1

    def direct(name: str) -> set[str]:
        return {
            dependency["name"]
            for dependency in packages[name]["dependencies"]
            if dependency["source"] is None
        }

    primitives = direct("eggserve-primitives")
    forbidden = {"eggserve-core", "eggserve-server", "eggserve-static", "hyper", "hyper-util", "quinn", "h3", "h3-quinn", "rustls", "tokio", "rustix", "windows-sys"}
    if primitives & forbidden:
        print(f"eggserve-primitives leaks forbidden dependencies: {sorted(primitives & forbidden)}", file=sys.stderr)
        return 1

    server = direct("eggserve-server")
    if "eggserve-core" in server or "eggserve-static" in server:
        print("eggserve-server must not depend on the compatibility core or static layer", file=sys.stderr)
        return 1
    if "eggserve-primitives" not in server:
        print("eggserve-server must depend on eggserve-primitives", file=sys.stderr)
        return 1

    static = direct("eggserve-static")
    if static != {"eggserve-primitives", "eggserve-server"}:
        print(f"unexpected eggserve-static direct dependencies: {sorted(static)}", file=sys.stderr)
        return 1

    print("Plan 211 crate topology: primitives leaf; server transport-only; static specializes both")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
