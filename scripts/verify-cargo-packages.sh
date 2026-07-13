#!/usr/bin/env bash
set -euo pipefail

# eggserve-bin depends on eggserve-core by path in the workspace. A normal
# crates.io publish dry-run cannot resolve that dependency until core has been
# published. This script stages a temporary publish-shaped workspace, then
# builds the exact generated `.crate` contents against the exact packaged core
# crate. A file-backed registry has no upload API, so `cargo publish --dry-run`
# is only used for the core crate; this package-and-build check is the
# documented bin equivalent. Nothing is uploaded to crates.io.

package_flags=(--locked)
if [ "${ALLOW_DIRTY:-false}" = "true" ]; then
  package_flags+=(--allow-dirty)
fi

assert_package_contents() {
  local package="$1"
  shift
  local listing
  listing="$(cargo package -p "$package" "${package_flags[@]}" --list)"
  for required in "$@"; do
    if ! grep -Fqx "$required" <<<"$listing"; then
      echo "$package package is missing $required" >&2
      exit 1
    fi
  done
  printf '%s\n' "$listing"
}

assert_package_contents eggserve-core \
  Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
cargo publish -p eggserve-core "${package_flags[@]}" --dry-run

core_version="$(cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name == "eggserve-core") | .version')"
core_crate="target/package/eggserve-core-${core_version}.crate"
if [ ! -f "$core_crate" ]; then
  echo "packaged core crate not found at $core_crate" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
validation_root="$tmp_dir/workspace"
mkdir -p "$validation_root/crates"
cp Cargo.toml Cargo.lock README.md LICENSE "$validation_root/"
cp -R crates/eggserve-core crates/eggserve-bin "$validation_root/crates/"

index_dir="$tmp_dir/index"
crate_dir="$tmp_dir/crates"
mkdir -p "$index_dir/eg/gs" "$crate_dir"
cp "$core_crate" "$crate_dir/"
core_checksum="$(sha256sum "$core_crate" | awk '{print $1}')"
core_index_entry="$(cargo metadata --format-version 1 --no-deps | jq -c \
  --arg checksum "$core_checksum" \
  '.packages[] | select(.name == "eggserve-core") | {
    name,
    vers: .version,
    deps: [.dependencies[] | {
      name,
      req,
      features,
      optional,
      default_features: .uses_default_features,
      target,
      kind: (.kind // "normal"),
      registry: "https://github.com/rust-lang/crates.io-index"
    }],
    cksum: $checksum,
    features,
    yanked: false,
    links: null
  }')"
printf '%s\n' "$core_index_entry" > "$index_dir/eg/gs/eggserve-core"
printf '{"dl":"file://%s/{crate}-{version}.crate"}\n' "$crate_dir" > "$index_dir/config.json"
git -C "$index_dir" init -q
git -C "$index_dir" config user.email release-validation@example.invalid
git -C "$index_dir" config user.name release-validation
git -C "$index_dir" add .
git -C "$index_dir" commit -q -m 'stage crates.io index for release validation'

# The checked-in manifest's path dependency resolves as a crates.io dependency
# once published. In this temporary workspace it is assigned to a local
# registry containing the exact core crate; all other dependencies remain on
# crates.io. This exercises the published package graph without uploading.
mkdir -p "$validation_root/.cargo"
printf '[registries.local]\nindex = "file://%s"\n' "$index_dir" > "$validation_root/.cargo/config.toml"
bin_manifest="$validation_root/crates/eggserve-bin/Cargo.toml"
sed -i.bak 's#eggserve-core = { path = "../eggserve-core", version = "0.1.0" }#eggserve-core = { version = "0.1.0", registry = "local" }#' "$bin_manifest"
rm -f "$bin_manifest.bak"
(cd "$validation_root" && cargo generate-lockfile)
bin_listing="$(cd "$validation_root" && cargo package -p eggserve-bin "${package_flags[@]}" --registry local --no-verify --list)"
for required in Cargo.toml Cargo.lock README.md LICENSE src/lib.rs src/main.rs; do
  if ! grep -Fqx "$required" <<<"$bin_listing"; then
    echo "eggserve-bin package is missing $required" >&2
    exit 1
  fi
done

cd "$validation_root"
cargo package -p eggserve-bin "${package_flags[@]}" --registry local --no-verify
bin_crate="target/package/eggserve-bin-${core_version}.crate"
if [ ! -f "$bin_crate" ]; then
  echo "packaged bin crate not found at $bin_crate" >&2
  exit 1
fi

core_unpack="$tmp_dir/core-unpacked"
bin_unpack="$tmp_dir/bin-unpacked"
mkdir -p "$core_unpack" "$bin_unpack"
tar -xzf "$OLDPWD/$core_crate" -C "$core_unpack"
tar -xzf "$bin_crate" -C "$bin_unpack"
core_source="$core_unpack/eggserve-core-${core_version}"
bin_source="$bin_unpack/eggserve-bin-${core_version}"
if grep -Fq 'path = "../eggserve-core"' "$bin_source/Cargo.toml"; then
  echo "packaged eggserve-bin manifest retained a repository-only path dependency" >&2
  exit 1
fi
sed -i.bak '/^registry-index = /d' "$bin_source/Cargo.toml"
rm -f "$bin_source/Cargo.toml.bak"
printf '\n[patch.crates-io]\neggserve-core = { path = "%s" }\n' "$core_source" >> "$bin_source/Cargo.toml"
cargo generate-lockfile --manifest-path "$bin_source/Cargo.toml" --offline
cargo build --manifest-path "$bin_source/Cargo.toml" --locked --offline

echo "Core passed a crates.io publish dry-run; bin passed equivalent packaged-graph verification."
