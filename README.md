# FnScribe

Private, push-to-talk dictation for macOS.

Hold a shortcut, speak, and release. FnScribe transcribes your voice locally and
types the result into the app you are using. It lives in the menu bar and stays
out of the way until you need it.

## Download

[Download FnScribe for Apple Silicon](https://github.com/AlgorithmicResearchGroup/fnscribe/releases/latest/download/FnScribe-arm64.dmg)

FnScribe requires an Apple Silicon Mac running macOS 13 Ventura or later.

1. Open the downloaded DMG and drag **FnScribe** to **Applications**.
2. Open FnScribe from your Applications folder. It appears in the menu bar, not
   the Dock.
3. Click the menu-bar microphone and grant **Microphone** and **Keyboard access**
   when prompted.
4. Hold the `fn`/Globe key, speak, and release to insert the transcription into
   the currently focused app.

## How to use it

The default push-to-talk shortcut is the `fn`/Globe key. To change it, click the
menu-bar microphone, click the shortcut button, and press a new key combination.

The menu-bar icon shows what FnScribe is doing:

- Microphone: ready
- Square: recording
- Three dots: transcribing
- Exclamation mark: permission or microphone attention needed

FnScribe currently transcribes English speech.

## Private by design

Speech recognition runs entirely on your Mac using a bundled Whisper model.
Audio and transcripts are held only in memory: there is no account, cloud
service, transcription history, or notes database.

FnScribe needs:

- **Microphone access** to record while you hold the shortcut
- **Accessibility (Keyboard access)** to detect the shortcut and insert text
  into the active app

## Troubleshooting

If FnScribe cannot record or insert text, click its menu-bar icon and check that
both permissions show **Granted**. You can also review them in **System Settings
→ Privacy & Security → Microphone** and **Accessibility**.

If macOS no longer recognizes a permission after you move the app, quit
FnScribe, keep it in Applications, and grant the permission again.

## Build from source

Building requires Rust 1.85 or newer, CMake, Clang, and the Tauri CLI:

```sh
cargo install tauri-cli --version '^2' --locked
./scripts/build-app.sh
```

The app bundle is written to
`target/release/bundle/macos/FnScribe.app`.

For development:

```sh
./scripts/download-model.sh
cargo tauri dev
```

To use a different whisper.cpp GGML model during development, set
`FNSCRIBE_MODEL` to its absolute path.
