#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
harness_target="$project_dir/target/delivery-harness"
app_path="$harness_target/release/bundle/macos/FnScribe.app"
result_file=$(mktemp /tmp/fnscribe-delivery-result.XXXXXX)
reuse_build=false
if [ "${1:-}" = "--reuse" ]; then
  reuse_build=true
  shift
fi
target_label=${1:-"an editable control"}
marker="FNSCRIBE_DELIVERY_$(date +%s)"

cleanup() {
  if [ -e "$result_file" ]; then
    /bin/unlink "$result_file"
  fi
}
trap cleanup EXIT HUP INT TERM

if pgrep -x fnscribe >/dev/null 2>&1; then
  echo "Quit the regular FnScribe app before running the delivery harness."
  exit 1
fi

if [ "$reuse_build" = false ]; then
  cd "$project_dir"
  ./scripts/download-model.sh
  CARGO_TARGET_DIR="$harness_target" cargo tauri build --bundles app --features delivery-harness

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
    codesign --force --deep --options runtime \
      --entitlements "$project_dir/src-tauri/Entitlements.plist" \
      --sign "$signing_identity" "$app_path"
  else
    codesign --force --deep --sign - --identifier com.arg.fnscribe.delivery-harness "$app_path"
  fi
  codesign --verify --deep --strict "$app_path"
elif [ ! -d "$app_path" ]; then
  echo "No reusable harness build exists. Run without --reuse first."
  exit 1
else
  codesign --verify --deep --strict "$app_path"
fi

echo "Focus $target_label within 3 seconds."
echo "Expected marker: $marker"
FNSCRIBE_DELIVERY_HARNESS_TEXT="$marker" \
FNSCRIBE_DELIVERY_HARNESS_RESULT="$result_file" \
  "$app_path/Contents/MacOS/fnscribe" &
harness_pid=$!
wait "$harness_pid" || true

if [ -s "$result_file" ]; then
  echo "Harness report:"
  /bin/cat "$result_file"
  echo
  if ! /usr/bin/grep -q '"success": true' "$result_file"; then
    exit 1
  fi
else
  echo "The harness exited without a report."
  exit 1
fi
