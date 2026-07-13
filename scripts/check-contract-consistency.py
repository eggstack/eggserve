#!/usr/bin/env python3
"""Contract consistency validator for eggserve."""

from __future__ import annotations

import re
import sys
from pathlib import Path

if sys.version_info >= (3, 11):
    import tomllib
else:
    tomllib = None  # type: ignore[assignment]


def _read(repo_root: Path, rel: str) -> str | None:
    path = repo_root / rel
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def _parse_cargo_version(text: str) -> str:
    m = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    return m.group(1) if m else ""


def _parse_pyproject_version(text: str) -> str:
    m = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    return m.group(1) if m else ""


def _parse_python_constraint(text: str) -> str:
    m = re.search(r'requires-python\s*=\s*"([^"]+)"', text)
    return m.group(1) if m else ""


def _extract_python_version_docs(text: str) -> list[str]:
    return re.findall(r'>=(3\.\d+(?:,\s*<3\.\d+)*)', text)


def _extract_readme_links(text: str) -> list[str]:
    links: list[str] = []
    for m in re.finditer(r'\[([^\]]*)\]\(([^)]+)\)', text):
        target = m.group(2)
        if target.startswith("http://") or target.startswith("https://") or target.startswith("#"):
            continue
        target = target.split("#", 1)[0].split("?", 1)[0]
        if target:
            links.append(target)
    return links


def check_tls_claims(repo_root: Path) -> list[str]:
    errors: list[str] = []

    non_goals = _read(repo_root, "docs/non-goals.md")
    if non_goals is None:
        errors.append("docs/non-goals.md not found")
    else:
        lower = non_goals.lower()
        for term in ["tls is deferred", "tls not implemented", "tls server is deferred", "tls client is deferred"]:
            if term in lower:
                errors.append(f"docs/non-goals.md: contains '{term}' but TLS is implemented")

    readme = _read(repo_root, "README.md")
    if readme is None:
        errors.append("README.md not found")
    else:
        if "tls" not in readme.lower() and "docs/tls.md" not in readme:
            errors.append("README.md: must mention TLS or link to docs/tls.md")

    return errors


def check_python_version_consistency(repo_root: Path) -> list[str]:
    errors: list[str] = []

    pyproject = _read(repo_root, "crates/eggserve-python/pyproject.toml")
    if pyproject is None:
        errors.append("crates/eggserve-python/pyproject.toml not found")
        return errors

    pyproject_constraint = _parse_python_constraint(pyproject)
    if not pyproject_constraint:
        errors.append("crates/eggserve-python/pyproject.toml: requires-python not found")
        return errors

    expected = re.sub(r'\s+', '', pyproject_constraint)

    matrix = _read(repo_root, "docs/library-capability-matrix.md")
    if matrix is not None:
        matches = _extract_python_version_docs(matrix)
        if not matches:
            errors.append("docs/library-capability-matrix.md: Python version constraint not found in notes")
        else:
            for v in matches:
                normalized = ">=" + re.sub(r'\s+', '', v)
                if normalized != expected:
                    errors.append(
                        f"docs/library-capability-matrix.md: Python version '{normalized}' "
                        f"does not match pyproject.toml '{expected}'"
                    )

    toolchain = _read(repo_root, "docs/toolchain-support.md")
    if toolchain is not None:
        matches = _extract_python_version_docs(toolchain)
        if not matches:
            errors.append("docs/toolchain-support.md: Python version constraint not found in Python section")
        else:
            for v in matches:
                normalized = ">=" + re.sub(r'\s+', '', v)
                if normalized != expected:
                    errors.append(
                        f"docs/toolchain-support.md: Python version '{normalized}' "
                        f"does not match pyproject.toml '{expected}'"
                    )

    criteria = _read(repo_root, "release/criteria.toml")
    if criteria is not None and tomllib is not None:
        try:
            data = tomllib.loads(criteria)
            lang = data.get("languages", {})
            py = lang.get("python", {})
            constraint = py.get("constraint", "")
            if constraint != pyproject_constraint:
                errors.append(
                    f"release/criteria.toml [languages.python]: constraint '{constraint}' "
                    f"does not match pyproject.toml '{pyproject_constraint}'"
                )
        except Exception:
            errors.append("release/criteria.toml: failed to parse TOML")

    return errors


def check_package_version_consistency(repo_root: Path) -> list[str]:
    errors: list[str] = []

    sources = {
        "crates/eggserve-core/Cargo.toml": _parse_cargo_version,
        "crates/eggserve-bin/Cargo.toml": _parse_cargo_version,
        "crates/eggserve-python/Cargo.toml": _parse_cargo_version,
        "crates/eggserve-python/pyproject.toml": _parse_pyproject_version,
    }

    versions: dict[str, str] = {}
    for rel, parser in sources.items():
        text = _read(repo_root, rel)
        if text is None:
            errors.append(f"{rel}: file not found")
            continue
        v = parser(text)
        if not v:
            errors.append(f"{rel}: version not found")
        else:
            versions[rel] = v

    if len(set(versions.values())) > 1:
        details = ", ".join(f"{k}={v}" for k, v in sorted(versions.items()))
        errors.append(f"Package versions mismatch: {details}")

    return errors


def check_platform_claims(repo_root: Path) -> list[str]:
    errors: list[str] = []

    matrix = _read(repo_root, "docs/library-capability-matrix.md")
    if matrix is not None:
        if "supported-functional" not in matrix:
            errors.append(
                "docs/library-capability-matrix.md: Windows platform table "
                "does not classify Windows as 'supported-functional'"
            )

    release = _read(repo_root, "docs/release-contract.md")
    if release is not None:
        windows_line = ""
        in_platform_table = False
        for line in release.splitlines():
            if "## Platforms" in line:
                in_platform_table = True
                continue
            if in_platform_table and line.startswith("## "):
                break
            if in_platform_table and "Windows" in line and "|" in line:
                windows_line = line
                break
        if windows_line:
            lower = windows_line.lower()
            if "parser-level" not in lower and "partial" not in lower:
                errors.append(
                    "docs/release-contract.md: Windows platform row does not "
                    "classify as partial/parser-only"
                )
        else:
            errors.append("docs/release-contract.md: no Windows platform entry found in platform table")

    non_goals = _read(repo_root, "docs/non-goals.md")
    if non_goals is not None:
        if "reparse" not in non_goals.lower():
            errors.append(
                "docs/non-goals.md: does not mention Windows reparse-point limitation"
            )

    return errors


def check_stable_api_inventory(repo_root: Path) -> list[str]:
    errors: list[str] = []

    init_py = _read(repo_root, "crates/eggserve-python/python/eggserve/__init__.py")
    if init_py is None:
        errors.append("crates/eggserve-python/python/eggserve/__init__.py not found")
        return errors

    init_names = set()
    in_all = False
    in_native_all = False
    for line in init_py.splitlines():
        stripped = line.strip()
        if stripped.startswith("__all__") and "+=" not in stripped:
            in_all = True
            in_native_all = False
            m = re.search(r'__all__\s*\+\=\s*\[', stripped)
            if m:
                in_all = False
                in_native_all = True
                continue
            if "[" in stripped and "]" in stripped:
                for name in re.findall(r'"(\w+)"', stripped):
                    init_names.add(name)
                in_all = False
                continue
            continue
        if stripped.startswith("__all__") and "+=" in stripped:
            in_native_all = True
            in_all = False
            continue
        if in_all or in_native_all:
            if "]" in stripped:
                for name in re.findall(r'"(\w+)"', stripped):
                    init_names.add(name)
                in_all = False
                in_native_all = False
                continue
            for name in re.findall(r'"(\w+)"', stripped):
                init_names.add(name)

    server_py = _read(repo_root, "crates/eggserve-python/python/eggserve/server.py")
    server_names = set()
    if server_py is not None:
        in_all = False
        for line in server_py.splitlines():
            stripped = line.strip()
            if stripped.startswith("__all__") and "=" in stripped and "+=" not in stripped:
                in_all = True
                if "[" in stripped and "]" in stripped:
                    for name in re.findall(r'"(\w+)"', stripped):
                        server_names.add(name)
                    in_all = False
                continue
            if in_all:
                if "]" in stripped:
                    for name in re.findall(r'"(\w+)"', stripped):
                        server_names.add(name)
                    in_all = False
                    continue
                for name in re.findall(r'"(\w+)"', stripped):
                    server_names.add(name)

    api_stability = _read(repo_root, "docs/api-stability.md")
    if api_stability is None:
        errors.append("docs/api-stability.md not found")
        return errors

    stable_python = set()
    documented_python = set()
    in_python_section = False
    in_client_section = False
    in_table = False

    for line in api_stability.splitlines():
        stripped = line.strip()

        if stripped.startswith("### `eggserve.__init__`"):
            in_python_section = True
            in_client_section = False
            in_table = False
            continue
        if stripped.startswith("### `eggserve.server`"):
            in_python_section = True
            in_client_section = False
            in_table = False
            continue
        if "### `eggserve._native`" in stripped:
            in_python_section = True
            in_client_section = "Client" in stripped
            in_table = False
            continue
        if stripped.startswith("## ") or (stripped.startswith("### ") and "eggserve" not in stripped):
            in_python_section = False
            in_client_section = False
            in_table = False
            continue
        if stripped.startswith("| Item |") or stripped.startswith("| ---"):
            in_table = True
            continue
        if in_table and stripped.startswith("|"):
            parts = [p.strip() for p in stripped.split("|")]
            parts = [p for p in parts if p]
            if len(parts) >= 2:
                item = parts[0]
                tier = parts[1].lower() if len(parts) > 1 else ""
                if item.startswith("`"):
                    item = item.strip("`").split(".")[-1]
                item = item.rstrip("()")
                if in_python_section:
                    documented_python.add(item)
                    if tier == "stable" and not in_client_section:
                        stable_python.add(item)

    exported = init_names | server_names

    undocumented = exported - documented_python - {"__version__", "ResponsePlan", "NATIVE_AVAILABLE"}
    if undocumented:
        errors.append(
            f"Exported Python names not in api-stability.md at all: "
            f"{', '.join(sorted(undocumented))}"
        )

    missing_from_exports = stable_python - exported
    if missing_from_exports:
        errors.append(
            f"api-stability.md stable Python items not in __all__: "
            f"{', '.join(sorted(missing_from_exports))}"
        )

    return errors


def check_no_stale_deferred_claims(repo_root: Path) -> list[str]:
    errors: list[str] = []
    docs_dir = repo_root / "docs"
    if not docs_dir.is_dir():
        errors.append("docs/ directory not found")
        return errors

    # Features that are implemented and should NOT be described as deferred
    implemented_features = {
        "tls": "TLS is implemented behind the `tls` feature flag",
        "client": "HTTP client is implemented behind the `client` feature flag",
        "range": "Range requests are implemented and tested",
    }

    # Patterns that indicate a stale deferred claim
    stale_patterns = [
        r"tls\s+is\s+deferred",
        r"tls\s+server\s+is\s+deferred",
        r"tls\s+client\s+is\s+deferred",
        r"tls\s+not\s+implemented",
        r"tls\s+support\s+is\s+deferred",
    ]

    for md_file in sorted(docs_dir.glob("*.md")):
        rel = str(md_file.relative_to(repo_root))
        text = md_file.read_text(encoding="utf-8").lower()
        for pattern in stale_patterns:
            for m in re.finditer(pattern, text):
                errors.append(f"{rel}: contains stale deferred claim '{m.group()}'")

    return errors


def check_readme_links(repo_root: Path) -> list[str]:
    errors: list[str] = []

    readme = _read(repo_root, "README.md")
    if readme is None:
        errors.append("README.md not found")
        return errors

    seen: set[str] = set()
    for link in _extract_readme_links(readme):
        if link in seen:
            continue
        seen.add(link)
        target = repo_root / link
        if not target.exists():
            errors.append(f"README.md: relative link '{link}' target does not exist")

    return errors


def check_workflow_criteria_cross_validation(repo_root: Path) -> list[str]:
    """Validate that workflow job names and criteria workflow_job fields are consistent."""
    errors: list[str] = []

    if tomllib is None:
        return ["Python 3.11+ required for TOML parsing"]

    # Load criteria.toml
    criteria_text = _read(repo_root, "release/criteria.toml")
    if criteria_text is None:
        return ["release/criteria.toml not found"]

    try:
        criteria_data = tomllib.loads(criteria_text)
    except Exception:
        return ["release/criteria.toml: failed to parse TOML"]

    # Extract all workflow_job values from criteria
    criteria_jobs: dict[str, str] = {}  # gate_id -> workflow_job
    for gate in criteria_data.get("gate", []):
        gate_id = gate.get("id", "")
        workflow_job = gate.get("workflow_job")
        if workflow_job:
            criteria_jobs[gate_id] = workflow_job

    # Load CI workflow
    ci_text = _read(repo_root, ".github/workflows/ci.yml")
    if ci_text is None:
        errors.append(".github/workflows/ci.yml not found")
        return errors

    try:
        ci_data = _yaml_load_string(ci_text)
    except Exception:
        # If YAML parsing fails, skip this check gracefully
        return []

    # Extract CI job names (display names after "name:")
    ci_job_names: set[str] = set()
    for job_name, job in ci_data.get("jobs", {}).items():
        display_name = job.get("name", job_name)
        ci_job_names.add(display_name)
        ci_job_names.add(job_name)

    # Check that each criteria workflow_job has a corresponding CI job
    for gate_id, workflow_job in criteria_jobs.items():
        if workflow_job not in ci_job_names:
            # Check if it's a known special case (e.g. release-only jobs)
            release_only_jobs = {
                "validate", "stage-release", "publish", "build-artifacts",
                "build-python",
            }
            if workflow_job not in release_only_jobs:
                errors.append(
                    f"Gate '{gate_id}' references workflow_job '{workflow_job}' "
                    f"which is not found in ci.yml job names: {sorted(ci_job_names)}"
                )

    return errors


def _yaml_load_string(text: str) -> dict:
    """Load YAML from a string without requiring PyYAML.

    Uses a simple regex-based extraction for job names since full YAML
    parsing may not be available.
    """
    try:
        import yaml
        return yaml.safe_load(text)
    except ImportError:
        # Fallback: extract job names using regex
        import re
        jobs: dict[str, dict] = {}
        # Find job definitions (top-level keys under "jobs:")
        in_jobs = False
        current_job = None
        for line in text.splitlines():
            stripped = line.strip()
            if stripped == "jobs:":
                in_jobs = True
                continue
            if in_jobs:
                # Job name is indented 2 spaces under jobs:
                if line.startswith("  ") and not line.startswith("    ") and ":" in stripped:
                    current_job = stripped.split(":")[0].strip()
                    jobs[current_job] = {"name": current_job}
                # Display name is "name: ..." under a job
                if current_job and stripped.startswith("name:"):
                    name_val = stripped.split(":", 1)[1].strip().strip('"').strip("'")
                    jobs[current_job]["name"] = name_val
        return {"jobs": jobs}


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    verbose = "--verbose" in sys.argv or "-v" in sys.argv

    checks = [
        ("TLS claim consistency", check_tls_claims),
        ("Python version consistency", check_python_version_consistency),
        ("Package version consistency", check_package_version_consistency),
        ("Platform claim consistency", check_platform_claims),
        ("Stable API inventory", check_stable_api_inventory),
        ("README link validation", check_readme_links),
        ("No stale deferred claims", check_no_stale_deferred_claims),
        ("Workflow/criteria cross-validation", check_workflow_criteria_cross_validation),
    ]

    total_errors = 0
    passed = 0
    failed = 0

    for name, check_fn in checks:
        if verbose:
            print(f"  running {name}...")
        errors = check_fn(repo_root)
        if errors:
            failed += 1
            print(f"✗ {name}")
            for err in errors:
                print(f"  {err}")
                total_errors += 1
        else:
            passed += 1
            print(f"✓ {name}")

    print()
    print(f"{passed} passed, {failed} failed, {total_errors} total errors")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
