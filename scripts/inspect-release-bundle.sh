#!/usr/bin/env bash
set -euo pipefail

ARTIFACT_DIR="${1:?usage: $0 ARTIFACT_DIR PROVENANCE_JSON COMMIT RUN_ID}"
PROVENANCE_JSON="${2:?usage: $0 ARTIFACT_DIR PROVENANCE_JSON COMMIT RUN_ID}"
EXPECTED_COMMIT="${3:?usage: $0 ARTIFACT_DIR PROVENANCE_JSON COMMIT RUN_ID}"
EXPECTED_RUN_ID="${4:?usage: $0 ARTIFACT_DIR PROVENANCE_JSON COMMIT RUN_ID}"

checksum_file="$(find "$ARTIFACT_DIR" -type f -name checksums-sha256.txt -print -quit)"
if [ -z "$checksum_file" ]; then
  echo "release bundle has no checksum manifest" >&2
  exit 1
fi

checksum_rel="${checksum_file#"$ARTIFACT_DIR"/}"
(cd "$ARTIFACT_DIR" && sha256sum --check "$checksum_rel")

expected_archives=(
  eggserve-x86_64-unknown-linux-gnu.tar.gz
  eggserve-aarch64-unknown-linux-gnu.tar.gz
  eggserve-x86_64-apple-darwin.tar.gz
  eggserve-aarch64-apple-darwin.tar.gz
  eggserve-x86_64-pc-windows-msvc.zip
)

for archive in "${expected_archives[@]}"; do
  archive_path="$(find "$ARTIFACT_DIR" -type f -name "$archive" -print -quit)"
  if [ -z "$archive_path" ]; then
    echo "release bundle is missing $archive" >&2
    exit 1
  fi

  case "$archive" in
    *.tar.gz)
      contents="$(tar -tzf "$archive_path")"
      for required in ./eggserve ./README.md ./LICENSE; do
        if ! grep -Fqx "$required" <<<"$contents"; then
          echo "$archive is missing $required" >&2
          exit 1
        fi
      done
      ;;
    *.zip)
      contents="$(unzip -Z1 "$archive_path")"
      for required in eggserve.exe README.md LICENSE; do
        if ! grep -Fqx "$required" <<<"$contents"; then
          echo "$archive is missing $required" >&2
          exit 1
        fi
      done
      ;;
  esac
done

wheel_path="$(find "$ARTIFACT_DIR" -type f -name '*.whl' -print -quit)"
if [ -z "$wheel_path" ]; then
  echo "release bundle has no Python wheel" >&2
  exit 1
fi
wheel_contents="$(unzip -Z1 "$wheel_path")"
if ! grep -Eq '^eggserve/bin/eggserve(\.exe)?$' <<<"$wheel_contents"; then
  echo "Python wheel does not contain the packaged CLI binary" >&2
  exit 1
fi

if [ "$(jq -r '.commit' "$PROVENANCE_JSON")" != "$EXPECTED_COMMIT" ]; then
  echo "provenance commit does not match the workflow commit" >&2
  exit 1
fi
if [ "$(jq -r '.run_id' "$PROVENANCE_JSON")" != "$EXPECTED_RUN_ID" ]; then
  echo "provenance run_id does not match the workflow run" >&2
  exit 1
fi

echo "Release bundle contents, checksums, wheel binary, and provenance verified."
