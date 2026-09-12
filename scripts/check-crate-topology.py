#!/usr/bin/env python3
"""Enforce the Plan 211/212/213 Cargo dependency topology.

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

    required = {
        "eggnet-tls",
        "eggserve-primitives",
        "eggserve-server",
        "eggserve-static",
        "eggserve-h3",
    }
    missing = required - packages.keys()
    if missing:
        print(f"missing Plan 211/212/213 packages: {', '.join(sorted(missing))}", file=sys.stderr)
        return 1

    def direct(name: str) -> set[str]:
        return {
            dependency["name"]
            for dependency in packages[name]["dependencies"]
            if dependency["source"] is None
        }

    def production(name: str) -> set[str]:
        return {
            dependency["name"]
            for dependency in packages[name]["dependencies"]
            if dependency["kind"] is None
        }

    primitives = direct("eggserve-primitives")
    forbidden = {"eggserve-core", "eggserve-server", "eggserve-static", "hyper", "hyper-util", "quinn", "h3", "h3-quinn", "rustls", "tokio", "rustix", "windows-sys"}
    if primitives & forbidden:
        print(f"eggserve-primitives leaks forbidden dependencies: {sorted(primitives & forbidden)}", file=sys.stderr)
        return 1

    neutral_tls = direct("eggnet-tls")
    declared_tls = {
        dependency["name"]
        for dependency in packages["eggnet-tls"]["dependencies"]
        if dependency["kind"] is None
    }
    forbidden_tls = {
        "eggserve-core",
        "eggserve-server",
        "eggserve-static",
        "eggserve-bin",
        "eggserve-python",
        "eggress-core",
        "eggfetch",
        "tokio",
        "hyper",
        "hyper-util",
        "quinn",
        "h3",
        "h3-quinn",
        "tracing",
    }
    if declared_tls & forbidden_tls:
        print(
            "eggnet-tls leaks application/transport dependencies: "
            f"{sorted(declared_tls & forbidden_tls)}",
            file=sys.stderr,
        )
        return 1
    if neutral_tls:
        print(
            "eggnet-tls must not depend on workspace application crates: "
            f"{sorted(neutral_tls)}",
            file=sys.stderr,
        )
        return 1
    if not {"rustls", "rustls-pki-types"}.issubset(declared_tls):
        print("eggnet-tls must declare rustls and rustls-pki-types", file=sys.stderr)
        return 1

    server = direct("eggserve-server")
    if "eggserve-core" in server or "eggserve-static" in server:
        print("eggserve-server must not depend on the compatibility core or static layer", file=sys.stderr)
        return 1
    if "eggserve-primitives" not in server:
        print("eggserve-server must depend on eggserve-primitives", file=sys.stderr)
        return 1
    server_h3 = production("eggserve-server") & {"h3", "h3-quinn", "quinn"}
    if server_h3:
        print(
            "eggserve-server must not directly depend on the H3/QUIC stack: "
            f"{sorted(server_h3)}",
            file=sys.stderr,
        )
        return 1

    static = direct("eggserve-static")
    if static != {"eggserve-primitives", "eggserve-server"}:
        print(f"unexpected eggserve-static direct dependencies: {sorted(static)}", file=sys.stderr)
        return 1

    h3 = production("eggserve-h3")
    expected_h3 = {"h3", "h3-quinn", "quinn"}
    if not expected_h3.issubset(h3):
        print(
            "eggserve-h3 must own the coordinated H3/QUIC direct dependencies: "
            f"missing {sorted(expected_h3 - h3)}",
            file=sys.stderr,
        )
        return 1
    if {"eggserve-server", "eggserve-static", "eggserve-primitives"} & h3:
        print(
            "eggserve-h3 must remain a transport dependency boundary, not a "
            "server/static/primitives implementation dependency",
            file=sys.stderr,
        )
        return 1

    if "eggnet-tls" not in direct("eggserve-core"):
        print("eggserve-core must consume the neutral eggnet-tls crate", file=sys.stderr)
        return 1
    core = direct("eggserve-core")
    core_h3 = {"h3", "h3-quinn", "quinn"} & core
    if core_h3:
        print(
            "eggserve-core must not directly depend on the H3/QUIC stack: "
            f"{sorted(core_h3)}",
            file=sys.stderr,
        )
        return 1

    print(
        "Plan 211/212/213 topology: primitives leaf; neutral TLS; "
        "server transport-only; static specializes both; H3/QUIC isolated"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
