#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
app_path="$project_dir/target/release/bundle/macos/FnScribe.app"
dmg_path=${1:-"$project_dir/target/release/bundle/dmg/FnScribe-arm64.dmg"}
dmg_dir=$(dirname "$dmg_path")

if [ ! -d "$app_path" ]; then
  echo "Missing $app_path. Run ./scripts/build-app.sh first." >&2
  exit 1
fi

codesign --verify --deep --strict "$app_path"

staging_dir=$(mktemp -d "${TMPDIR:-/tmp}/fnscribe-dmg.XXXXXX")
trap 'rm -rf "$staging_dir"' EXIT INT TERM

# mktemp uses mode 0700, which is too restrictive for a distributable volume.
chmod 755 "$staging_dir"
mkdir -p "$dmg_dir"
ditto "$app_path" "$staging_dir/FnScribe.app"
ln -s /Applications "$staging_dir/Applications"

hdiutil create \
  -volname "FnScribe" \
  -srcfolder "$staging_dir" \
  -format UDZO \
  -ov \
  "$dmg_path"

hdiutil verify "$dmg_path"
shasum -a 256 "$dmg_path"
echo "Created $dmg_path"
