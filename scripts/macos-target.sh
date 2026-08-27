#!/bin/sh

# Shared macOS target and artifact paths for the build, notarization, and DMG
# scripts. Source this file, then call `resolve_macos_target <arch> <project>`.
resolve_macos_target() {
  fnscribe_requested_arch=${1:-native}
  fnscribe_project_dir=$2

  case "$fnscribe_requested_arch" in
    native)
      fnscribe_host_target=$(rustc -vV | sed -n 's/^host: //p')
      case "$fnscribe_host_target" in
        aarch64-apple-darwin)
          FNSCRIBE_MACOS_TARGET=aarch64-apple-darwin
          FNSCRIBE_MACOS_ARCH=arm64
          FNSCRIBE_MACOS_LIPO_ARCH=arm64
          ;;
        x86_64-apple-darwin)
          FNSCRIBE_MACOS_TARGET=x86_64-apple-darwin
          FNSCRIBE_MACOS_ARCH=x64
          FNSCRIBE_MACOS_LIPO_ARCH=x86_64
          ;;
        *)
          echo "The native Rust target is not macOS: $fnscribe_host_target" >&2
          return 1
          ;;
      esac
      ;;
    arm64 | aarch64 | aarch64-apple-darwin)
      FNSCRIBE_MACOS_TARGET=aarch64-apple-darwin
      FNSCRIBE_MACOS_ARCH=arm64
      FNSCRIBE_MACOS_LIPO_ARCH=arm64
      ;;
    intel | x64 | x86_64 | x86_64-apple-darwin)
      FNSCRIBE_MACOS_TARGET=x86_64-apple-darwin
      FNSCRIBE_MACOS_ARCH=x64
      FNSCRIBE_MACOS_LIPO_ARCH=x86_64
      ;;
    *)
      echo "Unknown macOS architecture: $fnscribe_requested_arch" >&2
      echo "Use native, arm64, or x64." >&2
      return 1
      ;;
  esac

  FNSCRIBE_MACOS_BUNDLE_DIR="$fnscribe_project_dir/target/$FNSCRIBE_MACOS_TARGET/release/bundle"
  FNSCRIBE_MACOS_APP_PATH="$FNSCRIBE_MACOS_BUNDLE_DIR/macos/FnScribe.app"
  FNSCRIBE_MACOS_BINARY_PATH="$FNSCRIBE_MACOS_APP_PATH/Contents/MacOS/fnscribe"
  FNSCRIBE_MACOS_DMG_PATH="$fnscribe_project_dir/target/release/bundle/dmg/FnScribe-$FNSCRIBE_MACOS_ARCH.dmg"
  FNSCRIBE_MACOS_NOTARIZATION_PATH="$FNSCRIBE_MACOS_BUNDLE_DIR/macos/FnScribe-$FNSCRIBE_MACOS_ARCH-notarization.zip"
}

verify_macos_binary_architecture() {
  if [ ! -f "$FNSCRIBE_MACOS_BINARY_PATH" ]; then
    echo "Missing $FNSCRIBE_MACOS_BINARY_PATH" >&2
    return 1
  fi

  fnscribe_binary_arches=$(lipo -archs "$FNSCRIBE_MACOS_BINARY_PATH")
  case " $fnscribe_binary_arches " in
    *" $FNSCRIBE_MACOS_LIPO_ARCH "*) ;;
    *)
      echo "Expected a $FNSCRIBE_MACOS_LIPO_ARCH executable, found: $fnscribe_binary_arches" >&2
      return 1
      ;;
  esac
}
