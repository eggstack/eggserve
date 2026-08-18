#!/usr/bin/env python3
"""Validate the conformance matrix's schema and value domains."""

from pathlib import Path
import tomllib


REQUIRED = {
    "resource",
    "method",
    "conditional",
    "range",
    "file_state",
    "http_version",
    "connection",
    "expected_status",
    "body_forbidden",
    "connection_reuse",
}
METHODS = {"GET", "HEAD", "POST"}
CONNECTIONS = {"close", "keep_alive"}


def main() -> None:
    matrix_path = Path(__file__).resolve().parents[1] / "conformance" / "conformance_matrix.toml"
    with matrix_path.open("rb") as matrix_file:
        document = tomllib.load(matrix_file)

    entries = document.get("matrix")
    if not isinstance(entries, list) or not entries:
        raise SystemExit("conformance matrix has no [[matrix]] entries")

    for index, entry in enumerate(entries, start=1):
        missing = REQUIRED - entry.keys()
        if missing:
            raise SystemExit(f"matrix entry {index} is missing: {sorted(missing)}")
        if entry["method"] not in METHODS:
            raise SystemExit(f"matrix entry {index} has invalid method")
        if entry["connection"] not in CONNECTIONS:
            raise SystemExit(f"matrix entry {index} has invalid connection policy")
        if not isinstance(entry["expected_status"], int) or not 100 <= entry["expected_status"] <= 599:
            raise SystemExit(f"matrix entry {index} has invalid expected status")
        if not isinstance(entry["body_forbidden"], bool):
            raise SystemExit(f"matrix entry {index} has invalid body_forbidden value")
        if not isinstance(entry["connection_reuse"], bool):
            raise SystemExit(f"matrix entry {index} has invalid connection_reuse value")

    # Coverage check: every declared resource must appear at least once.
    declared_resources = {
        "direct_file",
        "directory_index",
        "root_index",
        "directory_listing",
        "missing",
        "denied",
    }
    exercised = {e["resource"] for e in entries}
    missing = declared_resources - exercised
    if missing:
        raise SystemExit(
            f"declared resources not exercised in any matrix entry: {sorted(missing)}"
        )

    print(f"validated {len(entries)} conformance matrix entries")


if __name__ == "__main__":
    main()
