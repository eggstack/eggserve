#!/usr/bin/env bash
set -euo pipefail

# The workspace packages have path dependencies. A normal crates.io publish
# dry-run cannot resolve those dependencies until the lower layers have been
# published. The Plan 211/212 path stages a temporary publish-shaped workspace in
# dependency order and validates the exact generated `.crate` contents through
# a file-backed local registry. Nothing is uploaded to crates.io.
#
# Without the layered crates, the legacy core/bin path below stages core and
# builds the exact generated `.crate` contents for the binary equivalent.
#
# --mode core   Only verify eggserve-core
# --mode bin    Only verify eggserve-bin (packages core first, as bin depends on it)
# --mode all    Verify both (default)

MODE="all"
PYTHON="${PYTHON:-python3}"
while [ $# -gt 0 ]; do
  case "$1" in
    --mode)
      MODE="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

case "$MODE" in
  core|bin|all) ;;
  *) echo "Invalid mode: $MODE (expected: core, bin, or all)" >&2; exit 1 ;;
esac

# Plans 211/212 add publishable path dependencies. Cargo's package preparation
# resolves those dependencies against a registry, so the old core/bin-only
# check cannot validate the graph until the new crates have been published.
# Stage the complete ordered graph in a temporary local registry instead.
if [ -f crates/eggserve-primitives/Cargo.toml ]; then
  layered_tmp_dir="$(mktemp -d)"
  layered_registry="$layered_tmp_dir/registry"
  layered_index="$layered_tmp_dir/index"
  layered_stage="$layered_tmp_dir/stage"
  trap 'rm -rf "$layered_tmp_dir"' EXIT
  mkdir -p "$layered_registry" "$layered_index" "$layered_stage/.cargo"
  printf '{"dl":"file://%s/{crate}-{version}.crate"}\n' "$layered_registry" > "$layered_index/config.json"
  git -C "$layered_index" init -q
  git -C "$layered_index" config user.email release-validation@example.invalid
  git -C "$layered_index" config user.name release-validation
  git -C "$layered_index" add .
  git -C "$layered_index" commit -q -m 'initialize local registry for layered package validation'
  export CARGO_REGISTRIES_LOCAL_INDEX="file://$layered_index"

  write_layered_root() {
    local package="$1"
    rm -rf "$layered_stage"
    mkdir -p "$layered_stage/crates/$package" "$layered_stage/.cargo"
    cp Cargo.toml README.md LICENSE "$layered_stage/"
    cp -R "crates/$package/." "$layered_stage/crates/$package/"
    cp -R architecture docs examples "$layered_stage/"
    printf '[workspace]\nmembers = ["crates/%s"]\nresolver = "2"\n\n[workspace.package]\nversion = "0.1.2"\nedition = "2021"\nlicense = "MIT"\nrepository = "https://github.com/eggstack/eggserve"\nhomepage = "https://github.com/eggstack/eggserve"\nkeywords = ["http", "static-file-server", "security", "hardened", "http-server"]\ncategories = ["web-programming::http-server"]\nrust-version = "1.88"\n\n[workspace.lints.rust]\nunsafe_code = "deny"\n\n[profile.dist]\ninherits = "release"\nopt-level = "z"\nlto = "fat"\ncodegen-units = 1\nstrip = "symbols"\n' "$package" > "$layered_stage/Cargo.toml"
    printf '[registries.local]\nindex = "file://%s"\n' "$layered_index" > "$layered_stage/.cargo/config.toml"
  }

  rewrite_layered_dependencies() {
    local package="$1"
    local manifest="$layered_stage/crates/$package/Cargo.toml"
    case "$package" in
      eggserve-server)
        sed -i 's#eggserve-primitives = { path = "../eggserve-primitives", version = "0.1.2" }#eggserve-primitives = { version = "0.1.2", registry = "local" }#' "$manifest"
        ;;
      eggserve-static)
        sed -i 's#eggserve-primitives = { path = "../eggserve-primitives", version = "0.1.2" }#eggserve-primitives = { version = "0.1.2", registry = "local" }#' "$manifest"
        sed -i 's#eggserve-server = { path = "../eggserve-server", version = "0.1.2" }#eggserve-server = { version = "0.1.2", registry = "local" }#' "$manifest"
        ;;
      eggserve-core)
        sed -i 's#eggnet-tls = { path = "../eggnet-tls", version = "0.1.2", optional = true, default-features = false }#eggnet-tls = { version = "0.1.2", registry = "local", optional = true, default-features = false }#' "$manifest"
        sed -i 's#eggserve-primitives = { path = "../eggserve-primitives", version = "0.1.2" }#eggserve-primitives = { version = "0.1.2", registry = "local" }#' "$manifest"
        sed -i 's#eggserve-server = { path = "../eggserve-server", version = "0.1.2" }#eggserve-server = { version = "0.1.2", registry = "local" }#' "$manifest"
        sed -i 's#eggserve-static = { path = "../eggserve-static", version = "0.1.2" }#eggserve-static = { version = "0.1.2", registry = "local" }#' "$manifest"
        sed -i 's#eggserve-h3 = { path = "../eggserve-h3", version = "0.1.2", optional = true }#eggserve-h3 = { version = "0.1.2", registry = "local", optional = true }#' "$manifest"
        ;;
      eggserve-bin)
        sed -i 's#eggserve-core = { path = "../eggserve-core", version = "0.1.2" }#eggserve-core = { version = "0.1.2", registry = "local" }#' "$manifest"
        sed -i 's#eggserve-h3 = { path = "../eggserve-h3", version = "0.1.2", optional = true }#eggserve-h3 = { version = "0.1.2", registry = "local", optional = true }#' "$manifest"
        ;;
    esac
  }

  write_layered_index_entry() {
    local package="$1"
    local crate_file="$2"
    local manifest="$3"
    local checksum metadata entry index_path
    checksum="$(sha256sum "$crate_file" | awk '{print $1}')"
    metadata="$(cargo metadata --manifest-path "$manifest" --format-version 1 --no-deps)"
    entry="$(METADATA="$metadata" PACKAGE="$package" CHECKSUM="$checksum" LOCAL_INDEX="$layered_index" "$PYTHON" -c '
import json
import os

metadata = json.loads(os.environ["METADATA"])
package = next(item for item in metadata["packages"] if item["name"] == os.environ["PACKAGE"])
local_index = "file://" + os.environ["LOCAL_INDEX"]
dependencies = []
for dependency in package["dependencies"]:
    registry = dependency.get("registry")
    dependencies.append({
        "name": dependency["name"],
        "req": dependency["req"],
        "features": dependency["features"],
        "optional": dependency["optional"],
        "default_features": dependency["uses_default_features"],
        "target": dependency.get("target"),
        "kind": dependency.get("kind") or "normal",
        "registry": None if registry == local_index else (registry or "https://github.com/rust-lang/crates.io-index"),
    })
entry = {
    "name": package["name"],
    "vers": package["version"],
    "deps": dependencies,
    "cksum": os.environ["CHECKSUM"],
    "features": package["features"],
    "yanked": False,
    "links": None,
}
print(json.dumps(entry, separators=(",", ":")))
')"
    case "${#package}" in
      1) index_path="1/$package" ;;
      2) index_path="2/$package" ;;
      3) index_path="3/${package:0:1}/$package" ;;
      *) index_path="${package:0:2}/${package:2:2}/$package" ;;
    esac
    mkdir -p "$layered_index/$(dirname "$index_path")"
    printf '%s\n' "$entry" > "$layered_index/$index_path"
    git -C "$layered_index" add .
    git -C "$layered_index" commit -q -m "add $package to local registry"
  }

  package_layered() {
    local package="$1"
    shift
    local listing crate_file
    write_layered_root "$package"
    rewrite_layered_dependencies "$package"
    (cd "$layered_stage" && cargo generate-lockfile)
    listing="$(cd "$layered_stage" && cargo package -p "$package" --allow-dirty --locked --registry local --no-verify --list)"
    for required in "$@"; do
      if ! grep -Fqx "$required" <<<"$listing"; then
        echo "$package package is missing $required" >&2
        exit 1
      fi
    done
    (cd "$layered_stage" && cargo package -p "$package" --allow-dirty --locked --registry local --no-verify)
    crate_file="$layered_stage/target/package/$package-0.1.2.crate"
    if [ ! -f "$crate_file" ]; then
      echo "$package package was not produced" >&2
      exit 1
    fi
    cp "$crate_file" "$layered_registry/"
    write_layered_index_entry "$package" "$crate_file" "$layered_stage/crates/$package/Cargo.toml"
  }

  case "$MODE" in
    core)
      package_layered eggnet-tls Cargo.toml Cargo.lock README.md LICENSE src/lib.rs tests/neutral_tls.rs
      package_layered eggserve-primitives Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-server Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-static Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-h3 Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-core Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      ;;
    bin)
      package_layered eggnet-tls Cargo.toml Cargo.lock README.md LICENSE src/lib.rs tests/neutral_tls.rs
      package_layered eggserve-primitives Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-server Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-static Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-h3 Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-core Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-bin Cargo.toml Cargo.lock README.md LICENSE src/lib.rs src/main.rs
      ;;
    all)
      package_layered eggnet-tls Cargo.toml Cargo.lock README.md LICENSE src/lib.rs tests/neutral_tls.rs
      package_layered eggserve-primitives Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-server Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-static Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-h3 Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-core Cargo.toml Cargo.lock README.md LICENSE src/lib.rs
      package_layered eggserve-bin Cargo.toml Cargo.lock README.md LICENSE src/lib.rs src/main.rs
      ;;
  esac
  echo "Layered crates passed local-registry package verification."
  exit 0
fi

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
cargo package -p eggserve-core "${package_flags[@]}"
cargo publish -p eggserve-core "${package_flags[@]}" --dry-run

if [ "$MODE" = "core" ]; then
  echo "eggserve-core passed a crates.io publish dry-run."
  exit 0
fi

core_version="$(cargo metadata --format-version 1 --no-deps | "$PYTHON" -c '
import json
import sys

package = next(
    package for package in json.load(sys.stdin)["packages"]
    if package["name"] == "eggserve-core"
)
print(package["version"])
')"
bin_version="$(cargo metadata --format-version 1 --no-deps | "$PYTHON" -c '
import json
import sys

package = next(
    package for package in json.load(sys.stdin)["packages"]
    if package["name"] == "eggserve-bin"
)
print(package["version"])
')"
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
core_index_entry="$(cargo metadata --format-version 1 --no-deps | CHECKSUM="$core_checksum" "$PYTHON" -c '
import json
import os
import sys

package = next(
    package for package in json.load(sys.stdin)["packages"]
    if package["name"] == "eggserve-core"
)
entry = {
    "name": package["name"],
    "vers": package["version"],
    "deps": [
        {
            "name": dependency["name"],
            "req": dependency["req"],
            "features": dependency["features"],
            "optional": dependency["optional"],
            "default_features": dependency["uses_default_features"],
            "target": dependency.get("target"),
            "kind": dependency.get("kind") or "normal",
            "registry": "https://github.com/rust-lang/crates.io-index",
        }
        for dependency in package["dependencies"]
    ],
    "cksum": os.environ["CHECKSUM"],
    "features": package["features"],
    "yanked": False,
    "links": None,
}
print(json.dumps(entry, separators=(",", ":")))
')"
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
sed -i.bak "s#eggserve-core = { path = \"../eggserve-core\", version = \"${core_version}\" }#eggserve-core = { version = \"${core_version}\", registry = \"local\" }#" "$bin_manifest"
rm -f "$bin_manifest.bak"
if ! grep -Fq 'eggserve-core = {' "$bin_manifest" || \
   ! grep -Fq 'registry = "local"' "$bin_manifest" || \
   grep -Fq 'path = "../eggserve-core"' "$bin_manifest"; then
  echo "failed to rewrite eggserve-bin's core dependency for the local registry" >&2
  exit 1
fi
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
bin_crate="target/package/eggserve-bin-${bin_version}.crate"
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
bin_source="$bin_unpack/eggserve-bin-${bin_version}"
if grep -Fq 'path = "../eggserve-core"' "$bin_source/Cargo.toml"; then
  echo "packaged eggserve-bin manifest retained a repository-only path dependency" >&2
  exit 1
fi
sed -i.bak '/^registry-index = /d' "$bin_source/Cargo.toml"
rm -f "$bin_source/Cargo.toml.bak"
printf '\n[patch.crates-io]\neggserve-core = { path = "%s" }\n' "$core_source" >> "$bin_source/Cargo.toml"
cargo generate-lockfile --manifest-path "$bin_source/Cargo.toml" --offline
cargo build --manifest-path "$bin_source/Cargo.toml" --locked --offline

if [ "$MODE" = "bin" ]; then
  echo "eggserve-bin passed equivalent packaged-graph verification."
  exit 0
fi

echo "Core passed a crates.io publish dry-run; bin passed equivalent packaged-graph verification."
