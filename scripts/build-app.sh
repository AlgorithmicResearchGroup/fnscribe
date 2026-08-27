#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
entitlements_path="$project_dir/src-tauri/Entitlements.plist"
architecture=${1:-native}

. "$script_dir/macos-target.sh"
resolve_macos_target "$architecture" "$project_dir"
app_path=$FNSCRIBE_MACOS_APP_PATH

if ! rustup target list --installed | grep -qx "$FNSCRIBE_MACOS_TARGET"; then
  echo "Rust target $FNSCRIBE_MACOS_TARGET is not installed." >&2
  echo "Install it with: rustup target add $FNSCRIBE_MACOS_TARGET" >&2
  exit 1
fi

cd "$project_dir"
./scripts/download-model.sh
cargo tauri build --bundles app --target "$FNSCRIBE_MACOS_TARGET"
verify_macos_binary_architecture

# Prefer a real signing identity so macOS can keep Accessibility and Microphone
# grants across rebuilds. Fall back to ad-hoc signing on Macs without one.
signing_identity=${FNSCRIBE_SIGNING_IDENTITY:-}
if [ -z "$signing_identity" ]; then
  signing_identity=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Developer ID Application:[^"]*\)"/\1/p' \
    | head -n 1)
fi
if [ -z "$signing_identity" ]; then
  signing_identity=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Apple Development:[^"]*\)"/\1/p' \
    | head -n 1)
fi

if [ -n "$signing_identity" ]; then
  codesign --force --deep --options runtime --entitlements "$entitlements_path" \
    --sign "$signing_identity" "$app_path"
else
  codesign --force --deep --sign - --identifier com.arg.fnscribe "$app_path"
fi
codesign --verify --deep --strict "$app_path"

echo "Built $FNSCRIBE_MACOS_ARCH app: $app_path"
