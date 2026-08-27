#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
architecture=${1:-native}
profile=${FNSCRIBE_NOTARY_PROFILE:-fnscribe-notary}

. "$script_dir/macos-target.sh"
resolve_macos_target "$architecture" "$project_dir"
app_path=$FNSCRIBE_MACOS_APP_PATH
submission_path=$FNSCRIBE_MACOS_NOTARIZATION_PATH

if [ ! -d "$app_path" ]; then
  echo "Missing $app_path. Run ./scripts/build-app.sh $architecture first." >&2
  exit 1
fi

verify_macos_binary_architecture
codesign --verify --deep --strict "$app_path"
if [ -e "$submission_path" ]; then
  /bin/unlink "$submission_path"
fi
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$submission_path"
xcrun notarytool submit "$submission_path" \
  --keychain-profile "$profile" \
  --wait \
  --timeout 30m
xcrun stapler staple "$app_path"
xcrun stapler validate "$app_path"
spctl --assess --type execute --verbose=4 "$app_path"

# Package the stapled app so the distributed image carries its ticket.
"$script_dir/package-dmg.sh" "$architecture"
