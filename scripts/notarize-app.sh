#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
app_path="$project_dir/target/release/bundle/macos/FnScribe.app"
submission_path="$project_dir/target/release/bundle/macos/FnScribe-notarization.zip"
release_path="$project_dir/target/release/bundle/macos/FnScribe.zip"
profile=${FNSCRIBE_NOTARY_PROFILE:-fnscribe-notary}

if [ ! -d "$app_path" ]; then
  echo "Missing $app_path. Run ./scripts/build-app.sh first." >&2
  exit 1
fi

codesign --verify --deep --strict "$app_path"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$submission_path"
xcrun notarytool submit "$submission_path" \
  --keychain-profile "$profile" \
  --wait \
  --timeout 30m
xcrun stapler staple "$app_path"
xcrun stapler validate "$app_path"
spctl --assess --type execute --verbose=4 "$app_path"

# Package the stapled app so the distributed archive carries its ticket.
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$release_path"
shasum -a 256 "$release_path"
echo "Notarized release: $release_path"
