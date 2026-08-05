# FnScribe

A deliberately small, local-only macOS dictation utility written in Rust.

Hold the configured global shortcut, speak, and release it to insert the
transcription into the currently focused application. Audio and transcripts are
kept only in memory. There is no history, notes database, account, or network
service.

## Requirements

- Apple Silicon Mac running macOS 13 or newer
- Rust 1.85 or newer
- CMake and Clang (used to build `whisper.cpp` through `whisper-rs`)

## Setup

Download the bundled English Whisper model once:

```sh
./scripts/download-model.sh
```

Install the Tauri CLI if it is not already installed:

```sh
cargo install tauri-cli --version '^2' --locked
```

Build and locally sign the macOS application bundle:

```sh
./scripts/build-app.sh
```

For distribution, save Apple notarization credentials in Keychain once:

```sh
xcrun notarytool store-credentials fnscribe-notary \
  --apple-id "YOUR_APPLE_ID" \
  --team-id "8432F4M85Y"
```

The command securely prompts for an app-specific password. Then notarize,
staple, validate with Gatekeeper, and create the distributable ZIP:

```sh
./scripts/notarize-app.sh
```

Then open:

```text
target/release/bundle/macos/FnScribe.app
```

On first use, macOS asks for Microphone and Accessibility permissions. The
latter is shown as "Keyboard access" in the settings pane. Keep the same bundle
identifier and application path so macOS retains those permissions.

## Development

```sh
cargo tauri dev
```

The default push-to-talk shortcut is the Fn/Globe key. Click the menu-bar
microphone and then the shortcut button to change it. The recorder accepts Fn
by itself or an ordinary modified shortcut.

## Model override

For development, set `FNSCRIBE_MODEL` to another whisper.cpp GGML model:

```sh
FNSCRIBE_MODEL=/absolute/path/to/model.bin cargo tauri dev
```
