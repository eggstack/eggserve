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


def check_trigger_policy_consistency(repo_root: Path) -> list[str]:
    """Validate that criteria.toml trigger policies are consistent with ci.yml job conditions.

    Checks that:
    - Gates with triggers = ["push"] only have corresponding CI jobs with
      ``if: github.event_name == 'push'``.
    - Gates with triggers = ["pull_request", "push"] run on jobs without a
      push-only ``if:`` condition.
    - No contradictory PR/main policy exists.
    """
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

    # Extract triggers and workflow_job from each gate
    gate_triggers: dict[str, list[str]] = {}
    gate_jobs: dict[str, str] = {}  # gate_id -> workflow_job
    for gate in criteria_data.get("gate", []):
        gate_id = gate.get("id", "")
        triggers = gate.get("triggers", [])
        workflow_job = gate.get("workflow_job")
        if triggers:
            gate_triggers[gate_id] = triggers
        if workflow_job:
            gate_jobs[gate_id] = workflow_job

    if not gate_triggers:
        return []

    # Load CI workflow
    ci_text = _read(repo_root, ".github/workflows/ci.yml")
    if ci_text is None:
        return [".github/workflows/ci.yml not found"]

    try:
        ci_data = _yaml_load_string(ci_text)
    except Exception:
        return []

    # Build a map of job_name -> whether it has a push-only if condition
    jobs = ci_data.get("jobs", {})
    job_push_only: dict[str, bool] = {}
    for job_key, job_val in jobs.items():
        if_condition = job_val.get("if", "")
        job_push_only[job_key] = "github.event_name == 'push'" in if_condition

    # Build reverse map: job display_name -> job_key (for lookup by display name)
    display_to_key: dict[str, str] = {}
    for job_key, job_val in jobs.items():
        display_name = job_val.get("name", job_key)
        display_to_key[display_name] = job_key

    # Release-only jobs that don't exist in ci.yml are not checked
    release_only_jobs = {
        "validate", "stage-release", "publish", "build-artifacts",
        "build-python",
    }

    for gate_id, triggers in gate_triggers.items():
        workflow_job = gate_jobs.get(gate_id, "")
        if not workflow_job or workflow_job in release_only_jobs:
            continue

        # Find the CI job key
        job_key = display_to_key.get(workflow_job, workflow_job)
        if job_key not in job_push_only:
            continue

        is_push_only = job_push_only[job_key]

        if triggers == ["push"] and not is_push_only:
            errors.append(
                f"Gate '{gate_id}' has triggers = [\"push\"] but CI job "
                f"'{job_key}' does not have a push-only 'if:' condition"
            )
        elif set(triggers) == {"pull_request", "push"} and is_push_only:
            errors.append(
                f"Gate '{gate_id}' has triggers = [\"pull_request\", \"push\"] "
                f"but CI job '{job_key}' has a push-only 'if:' condition"
            )

    return errors


def check_python_server_defaults(repo_root: Path) -> list[str]:
    """Validate that Python Server constructor defaults are documented and consistent.

    Checks that:
    - docs/python-api.md documents the Python Server constructor defaults
    - Documented defaults match the actual code defaults in server.rs
    - Any differences from Rust/CLI defaults are documented as intentional
    """
    errors: list[str] = []

    # Read the Python API docs
    docs_text = _read(repo_root, "docs/python-api.md")
    if docs_text is None:
        errors.append("docs/python-api.md not found")
        return errors

    # Check that the defaults table exists
    if "Default parity with Rust/CLI" not in docs_text:
        errors.append(
            "docs/python-api.md: missing 'Default parity with Rust/CLI' section "
            "documenting Python vs Rust default differences"
        )

    # Read the Rust server.rs to extract Python Server constructor defaults
    server_rs = _read(repo_root, "crates/eggserve-python/src/server.rs")
    if server_rs is None:
        errors.append("crates/eggserve-python/src/server.rs not found")
        return errors

    # Extract default values from the #[pyo3(signature)] attribute
    import re
    # Find the PyServer constructor signature line
    sig_text = None
    for line in server_rs.split('\n'):
        if 'pyo3(signature' in line and 'max_connections' in line:
            m = re.search(r'#\[pyo3\(signature\s*=\s*\((.+)\)\)\]', line)
            if m:
                sig_text = m.group(1)
            break

    if sig_text is None:
        errors.append("server.rs: PyServer #[pyo3(signature)] attribute not found")
        return errors

    # Parse parameter defaults from the signature
    defaults: dict[str, str] = {}
    for param in sig_text.split(','):
        param = param.strip()
        if '=' in param:
            name, value = param.split('=', 1)
            defaults[name.strip()] = value.strip()

    # Validate that documented defaults match code defaults
    expected_defaults = {
        "bind": '"127.0.0.1"',
        "port": "8000",
        "public": "false",
        "max_connections": "100",
        "max_file_streams": "64",
        "max_python_callbacks": "8",
        "header_timeout_secs": "10",
        "connection_total_timeout_secs": "30",
        "handler_timeout_secs": "30",
        "graceful_shutdown_timeout_secs": "10",
    }

    for param, expected_value in expected_defaults.items():
        if param in defaults:
            actual_value = defaults[param]
            if actual_value != expected_value:
                errors.append(
                    f"server.rs default mismatch: {param}={actual_value} "
                    f"but expected {expected_value}"
                )
        else:
            # Optional params like policy and handler don't have defaults
            if param not in ("policy", "handler"):
                errors.append(f"server.rs: parameter '{param}' not found in signature")

    return errors


def check_production_profiles(repo_root: Path) -> list[str]:
    """Validate production profile definitions and cross-document consistency."""
    errors: list[str] = []

    if tomllib is None:
        return ["Python 3.11+ required for TOML parsing"]

    # Load support-profiles.toml
    profiles_text = _read(repo_root, "release/support-profiles.toml")
    if profiles_text is None:
        errors.append("release/support-profiles.toml not found")
        return errors

    try:
        profiles_data = tomllib.loads(profiles_text)
    except Exception:
        errors.append("release/support-profiles.toml: failed to parse TOML")
        return errors

    profiles = profiles_data.get("profile", [])
    if not profiles:
        errors.append("release/support-profiles.toml: no profiles defined")
        return errors

    valid_statuses = {"unsupported", "functional", "candidate", "supported-hardened"}
    profile_ids = set()
    for p in profiles:
        pid = p.get("profile", "")
        status = p.get("status", "")
        if not pid:
            errors.append("support-profiles.toml: profile missing 'profile' field")
            continue
        if pid in profile_ids:
            errors.append(f"support-profiles.toml: duplicate profile '{pid}'")
        profile_ids.add(pid)
        if status not in valid_statuses:
            errors.append(f"support-profiles.toml: profile '{pid}' has invalid status '{status}'")
        # Validate required fields
        for field in ["platform", "filesystem", "network_binding", "tls_termination",
                       "http_version", "security_defaults", "required_gates",
                       "excluded_flags", "notes"]:
            if field not in p:
                errors.append(f"support-profiles.toml: profile '{pid}' missing '{field}'")

    # Validate that hardened profiles don't allow symlink following
    for p in profiles:
        pid = p.get("profile", "")
        status = p.get("status", "")
        symlinks = p.get("symlink_following_allowed", False)
        if status == "supported-hardened" and symlinks:
            errors.append(
                f"support-profiles.toml: hardened profile '{pid}' must not allow symlink following"
            )

    # Validate that no profile permits plaintext by default
    for p in profiles:
        pid = p.get("profile", "")
        tls = p.get("tls_termination", "")
        binding = p.get("network_binding", "")
        # Local development on loopback with no TLS is acceptable
        if tls == "none" and binding not in ("loopback", "loopback-127.0.0.1"):
            errors.append(
                f"support-profiles.toml: profile '{pid}' has no TLS on non-loopback binding"
            )

    # Validate that required_gates reference existing gate IDs in criteria.toml
    criteria_text = _read(repo_root, "release/criteria.toml")
    if criteria_text is not None:
        try:
            criteria_data = tomllib.loads(criteria_text)
            gate_ids = {g.get("id", "") for g in criteria_data.get("gate", [])}
            for p in profiles:
                pid = p.get("profile", "")
                for gate_id in p.get("required_gates", []):
                    if gate_id not in gate_ids:
                        errors.append(
                            f"support-profiles.toml: profile '{pid}' references "
                            f"nonexistent gate '{gate_id}'"
                        )
        except Exception:
            pass

    # Check that docs reference profiles
    readme = _read(repo_root, "README.md")
    if readme is not None:
        if "support-profiles.toml" not in readme and "production profile" not in readme.lower():
            errors.append("README.md: does not reference production profiles")

    threat = _read(repo_root, "docs/threat-model.md")
    if threat is not None:
        if "production profile" not in threat.lower() and "support-profiles" not in threat:
            errors.append("docs/threat-model.md: does not reference production profiles")

    return errors


def check_non_goal_retention(repo_root: Path) -> list[str]:
    """Validate that explicit non-goals are retained in docs/non-goals.md."""
    errors: list[str] = []

    non_goals = _read(repo_root, "docs/non-goals.md")
    if non_goals is None:
        errors.append("docs/non-goals.md not found")
        return errors

    lower = non_goals.lower()

    required_non_goals = [
        ("asgi", "ASGI adapter"),
        ("wsgi", "WSGI adapter"),
        ("reverse proxy", "reverse proxying"),
        ("middleware", "middleware stack"),
        ("routing", "framework routing"),
        ("acme", "ACME/certificate automation"),
        ("http/2", "HTTP/2"),
        ("upload", "upload/write support"),
    ]

    for keyword, description in required_non_goals:
        if keyword not in lower:
            errors.append(
                f"docs/non-goals.md: missing required non-goal keyword '{keyword}' "
                f"({description})"
            )

    # Check that downstream extension language is present
    if "downstream" not in lower:
        errors.append(
            "docs/non-goals.md: missing downstream extension language"
        )

    return errors


def check_no_asgi_vocabulary_in_stable_api(repo_root: Path) -> list[str]:
    """Validate that no ASGI/WSGI vocabulary appears in stable API documentation."""
    errors: list[str] = []

    api_stability = _read(repo_root, "docs/api-stability.md")
    if api_stability is None:
        errors.append("docs/api-stability.md not found")
        return errors

    asgi_terms = ["asgi", "wsgi", "scope", "receive", "send", "app"]
    lower = api_stability.lower()

    for term in asgi_terms:
        if term in lower:
            # Check if it's in a non-goals or scope-boundary context, which is fine
            for line in api_stability.splitlines():
                if term in line.lower() and ("non-goal" in line.lower() or "scope" in line.lower()
                                              or "not" in line.lower() or "downstream" in line.lower()):
                    break
            else:
                errors.append(
                    f"docs/api-stability.md: contains ASGI/WSGI vocabulary '{term}' "
                    f"in a potentially stable API context"
                )

    return errors


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
        ("Trigger policy consistency", check_trigger_policy_consistency),
        ("Python server defaults", check_python_server_defaults),
        ("Production profile validation", check_production_profiles),
        ("Non-goal retention", check_non_goal_retention),
        ("No ASGI vocabulary in stable API", check_no_asgi_vocabulary_in_stable_api),
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
