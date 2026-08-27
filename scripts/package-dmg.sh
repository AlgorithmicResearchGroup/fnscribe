#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
architecture=${1:-native}
dmg_path=${2:-}

# Preserve the old `package-dmg.sh /path/to/output.dmg` form for native builds.
case "$architecture" in
  *.dmg)
    dmg_path=$architecture
    architecture=native
    ;;
esac

. "$script_dir/macos-target.sh"
resolve_macos_target "$architecture" "$project_dir"
app_path=$FNSCRIBE_MACOS_APP_PATH
dmg_path=${dmg_path:-$FNSCRIBE_MACOS_DMG_PATH}
dmg_dir=$(dirname "$dmg_path")

if [ ! -d "$app_path" ]; then
  echo "Missing $app_path. Run ./scripts/build-app.sh $architecture first." >&2
  exit 1
fi

verify_macos_binary_architecture
codesign --verify --deep --strict "$app_path"

staging_dir=$(mktemp -d "${TMPDIR:-/tmp}/fnscribe-dmg.XXXXXX")
trap 'rm -rf "$staging_dir"' EXIT INT TERM

# mktemp uses mode 0700, which is too restrictive for a distributable volume.
chmod 755 "$staging_dir"
mkdir -p "$dmg_dir"
ditto "$app_path" "$staging_dir/FnScribe.app"
ditto "$project_dir/LICENSE" "$staging_dir/LICENSE"
ln -s /Applications "$staging_dir/Applications"

hdiutil create \
  -volname "FnScribe" \
  -srcfolder "$staging_dir" \
  -format UDZO \
  -ov \
  "$dmg_path"

hdiutil verify "$dmg_path"
shasum -a 256 "$dmg_path"
echo "Created $FNSCRIBE_MACOS_ARCH installer: $dmg_path"
