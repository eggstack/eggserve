#!/usr/bin/env bash
# SBOM and provenance generation script (Plan 089, Track I).
#
# Produces:
# - SBOM for release artifacts
# - Checksums binding artifact hashes to source SHA
# - Provenance record including tag, commit, run ID, and timestamp
#
# Usage: bash scripts/generate-sbom.sh [--output-dir <dir>]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$REPO_ROOT/release/sbom"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

mkdir -p "$OUTPUT_DIR"

# Get source information
SOURCE_SHA=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
SOURCE_TAG=$(git describe --tags --exact-match 2>/dev/null || echo "untagged")
GIT_RUN_ID="${GITHUB_RUN_ID:-local}"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

echo "=== SBOM Generation ==="
echo "Source SHA: $SOURCE_SHA"
echo "Source Tag: $SOURCE_TAG"
echo "Output Dir: $OUTPUT_DIR"
echo ""

# Generate Rust dependency inventory
echo "Generating Rust dependency inventory..."
if command -v cargo &>/dev/null; then
    cargo metadata --format-version=1 > "$OUTPUT_DIR/cargo-metadata.json" 2>/dev/null || true
    echo "  Created cargo-metadata.json"
else
    echo "  SKIP: cargo not found"
fi

# Generate Python dependency inventory
echo "Generating Python dependency inventory..."
if command -v pip &>/dev/null; then
    pip list --format=json > "$OUTPUT_DIR/pip-packages.json" 2>/dev/null || true
    echo "  Created pip-packages.json"
else
    echo "  SKIP: pip not found"
fi

# Generate cargo audit results
echo "Running cargo audit..."
if command -v cargo-audit &>/dev/null; then
    cargo audit --json > "$OUTPUT_DIR/cargo-audit.json" 2>/dev/null || true
    echo "  Created cargo-audit.json"
else
    echo "  SKIP: cargo-audit not found (install with: cargo install cargo-audit)"
fi

# Generate cargo deny results
echo "Running cargo deny..."
if command -v cargo-deny &>/dev/null; then
    cargo deny check --format json > "$OUTPUT_DIR/cargo-deny.json" 2>/dev/null || true
    echo "  Created cargo-deny.json"
else
    echo "  SKIP: cargo-deny not found (install with: cargo install cargo-deny)"
fi

# Generate checksums for release artifacts
echo "Generating checksums..."
CHECKSUMS_FILE="$OUTPUT_DIR/checksums-sha256.txt"
> "$CHECKSUMS_FILE"

# Check for built binaries
if [[ -f "$REPO_ROOT/target/release/eggserve" ]]; then
    sha256sum "$REPO_ROOT/target/release/eggserve" >> "$CHECKSUMS_FILE"
    echo "  Added eggserve binary"
fi

# Check for Python wheels
for wheel in "$REPO_ROOT"/crates/eggserve-python/dist/*.whl; do
    if [[ -f "$wheel" ]]; then
        sha256sum "$wheel" >> "$CHECKSUMS_FILE"
        echo "  Added $(basename "$wheel")"
    fi
done

# Check for source archives
for archive in "$REPO_ROOT"/release/*.tar.gz "$REPO_ROOT"/release/*.zip; do
    if [[ -f "$archive" ]]; then
        sha256sum "$archive" >> "$CHECKSUMS_FILE"
        echo "  Added $(basename "$archive")"
    fi
done

if [[ -s "$CHECKSUMS_FILE" ]]; then
    echo "  Created checksums-sha256.txt"
else
    echo "  No artifacts found to checksum"
fi

# Generate SBOM
echo "Generating SBOM..."
SBOM_FILE="$OUTPUT_DIR/sbom.json"
CARGO_META="$OUTPUT_DIR/cargo-metadata.json"

if [[ -f "$CARGO_META" ]] && command -v jq &>/dev/null; then
    # Parse all packages from cargo metadata into SBOM components
    jq --arg tag "$SOURCE_TAG" --arg ts "$TIMESTAMP" '
    {
      "bomFormat": "CycloneDX",
      "specVersion": "1.4",
      "version": 1,
      "metadata": {
        "timestamp": $ts,
        "tools": [
          {
            "vendor": "eggserve",
            "name": "generate-sbom.sh",
            "version": "1.0.0"
          }
        ],
        "component": {
          "type": "application",
          "name": "eggserve",
          "version": $tag,
          "description": "Security-oriented static file server"
        }
      },
      "components": [
        .packages[] | {
          "type": "library",
          "name": .name,
          "version": .version,
          "description": (.description // ""),
          "licenses": [if .license then { "id": .license } else { "id": "UNKNOWN" } end],
          "purl": ("pkg:cargo/" + .name + "@" + .version),
          "properties": [
            { "name": "cargo:manifest_path", "value": .manifest_path },
            { "name": "cargo:source", "value": (.source // "local") }
          ]
        }
      ],
      "dependencies": [
        .resolve.nodes[] | {
          "ref": .id,
          "dependsOn": [.dependencies[]]
        }
      ]
    }
    ' "$CARGO_META" > "$SBOM_FILE"
    COMPONENT_COUNT=$(jq '.components | length' "$SBOM_FILE")
    echo "  Created sbom.json ($COMPONENT_COUNT components)"
else
    # Fallback: minimal SBOM without parsed components
    cat > "$SBOM_FILE" <<EOF
{
  "bomFormat": "CycloneDX",
  "specVersion": "1.4",
  "version": 1,
  "metadata": {
    "timestamp": "$TIMESTAMP",
    "tools": [
      {
        "vendor": "eggserve",
        "name": "generate-sbom.sh",
        "version": "1.0.0"
      }
    ],
    "component": {
      "type": "application",
      "name": "eggserve",
      "version": "$SOURCE_TAG",
      "description": "Security-oriented static file server"
    }
  },
  "components": [],
  "dependencies": [],
  "_warning": "Components not populated: cargo-metadata.json missing or jq unavailable"
}
EOF
    echo "  Created sbom.json (minimal, components not populated)"
fi

# Generate provenance record
echo "Generating provenance record..."
PROVENANCE_FILE="$OUTPUT_DIR/provenance.json"
cat > "$PROVENANCE_FILE" <<EOF
{
  "_type": "https://in-toto.io/Statement/v0.1",
  "predicateType": "https://slsa.dev/provenance/v0.2",
  "subject": [
    {
      "name": "eggserve",
      "digest": {
        "sha256": "$(sha256sum "$CHECKSUMS_FILE" 2>/dev/null | cut -d' ' -f1 || echo "pending")"
      }
    }
  ],
  "predicate": {
    "builder": {
      "id": "https://github.com/eggstack/eggserve"
    },
    "buildType": "https://github.com/eggstack/eggserve/build",
    "externalParameters": {
      "source": {
        "uri": "git+https://github.com/eggstack/eggserve.git@$SOURCE_SHA",
        "digest": {
          "sha1": "$SOURCE_SHA"
        }
      }
    },
    "internalParameters": {
      "git_run_id": "$GIT_RUN_ID",
      "source_tag": "$SOURCE_TAG",
      "timestamp": "$TIMESTAMP"
    },
    "metadata": {
      "buildInvocationId": "$GIT_RUN_ID",
      "completedOn": "$TIMESTAMP",
      "reproducible": true
    }
  }
}
EOF
echo "  Created provenance.json"

# Generate source archive identity
echo "Generating source archive identity..."
SOURCE_IDENTITY="$OUTPUT_DIR/source-identity.json"
cat > "$SOURCE_IDENTITY" <<EOF
{
  "source_sha1": "$SOURCE_SHA",
  "source_tag": "$SOURCE_TAG",
  "git_run_id": "$GIT_RUN_ID",
  "timestamp": "$TIMESTAMP",
  "git_commit_date": "$(git log -1 --format=%cI 2>/dev/null || echo "unknown")",
  "git_author": "$(git log -1 --format=%an 2>/dev/null || echo "unknown")",
  "git_message": "$(git log -1 --format=%s 2>/dev/null | head -c 200 || echo "unknown")",
  "rust_toolchain": "$(rustc --version 2>/dev/null || echo "unknown")",
  "python_version": "$(python3 --version 2>/dev/null || echo "unknown")",
  "cargo_version": "$(cargo --version 2>/dev/null || echo "unknown")"
}
EOF
echo "  Created source-identity.json"

# Generate reproducibility notes
echo "Generating reproducibility notes..."
REPRO_NOTES="$OUTPUT_DIR/reproducibility-notes.md"
cat > "$REPRO_NOTES" <<EOF
# Reproducibility Notes

## Build Environment

- **Source SHA**: $SOURCE_SHA
- **Source Tag**: $SOURCE_TAG
- **Timestamp**: $TIMESTAMP
- **Rust Toolchain**: $(rustc --version 2>/dev/null || echo "unknown")
- **Python Version**: $(python3 --version 2>/dev/null || echo "unknown")
- **Cargo Version**: $(cargo --version 2>/dev/null || echo "unknown")

## Build Steps

1. Clone source at $SOURCE_SHA
2. Build with: \`cargo build --release --locked\`
3. Build Python wheel with: \`maturin build --release\`
4. Generate checksums and SBOM

## Reproducibility Notes

- Cargo.lock is committed and must be used for builds
- Python wheel builds use maturin with deterministic settings
- No codegen or build-time code generation
- All dependencies are pinned in Cargo.lock

## Known Limitations

- Timestamps in provenance records vary by build
- Platform-specific binaries require native compilation
- Python wheel builds require the target Python version
EOF
echo "  Created reproducibility-notes.md"

echo ""
echo "=== SBOM Generation Complete ==="
echo "Output directory: $OUTPUT_DIR"
ls -la "$OUTPUT_DIR"
