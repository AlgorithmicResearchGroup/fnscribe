# FnScribe

Private, reliable dictation for macOS.

[Website](https://algorithmicresearchgroup.github.io/fnscribe/) ·
[Source](https://github.com/AlgorithmicResearchGroup/fnscribe)

Hold a shortcut, speak, and release—or use hands-free mode for longer thoughts.
FnScribe transcribes your voice locally and inserts the result into the app you
are using. It lives in the menu bar and stays out of the way until you need it.

## Download

[Download for Apple Silicon](https://github.com/AlgorithmicResearchGroup/fnscribe/releases/latest/download/FnScribe-arm64.dmg) ·
[Download for Intel Mac](https://github.com/AlgorithmicResearchGroup/fnscribe/releases/latest/download/FnScribe-x64.dmg)

FnScribe requires an Apple Silicon or 64-bit Intel Mac running macOS 13 Ventura
or later.

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

For hands-free dictation, press `fn + Space` once to start and again to stop. You
can also turn a push-to-talk recording into hands-free mode by pressing Space
while continuing to hold `fn`. Press `Escape` at any point to cancel without
inserting text. The compact flowbar shows the active stage and provides Stop and
Cancel controls. Recordings stop automatically after two minutes.

The settings panel lets you choose a microphone and launch FnScribe at login.
Smart cleanup is enabled by default. It removes conservative fillers (`um`,
`uh`, and `erm`) and understands explicit formatting such as `comma`, `period`,
`question mark`, `new line`, and `new paragraph`. Repeated `number one`,
`number two` or `bullet` phrases become lists. Say `scratch that` to discard the
current clause; for short value corrections, say `two, actually three`, or use
`actually make that` for an explicit replacement.

The personal dictionary supplies local recognition hints and preserves exact
spelling and capitalization. Each entry has a **Write as** value and an optional
**Common mishearing** replacement—for example, `FnScribe` and `fn scribe`.
Dictionary entries are stored only in FnScribe's local settings file, with
owner-only file permissions, and are never transmitted.

If automatic insertion ever fails, use **Copy Last Transcript** or **Paste Last
Transcript** from the menu-bar menu. That recovery transcript exists only in
memory and disappears when FnScribe quits. If smart cleanup changed the last
dictation, **Original** copies the unmodified transcription for recovery.

The menu-bar icon shows what FnScribe is doing:

- Microphone: ready
- Square: recording
- Three dots: starting, transcribing, or inserting
- Exclamation mark: permission or microphone attention needed

FnScribe currently transcribes English speech.

## Private by design

Speech recognition runs entirely on your Mac using a bundled Whisper model.
Audio and transcripts are held only in memory: there is no account, cloud
service, transcription history, or notes database.

For broad compatibility, automatic insertion uses a short-lived, concealed
clipboard entry and immediately restores the previous clipboard when it has not
changed in the meantime. The transcript remains available only in memory for
recovery; choosing **Copy Last Transcript** is the only persistent clipboard
action.

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
./scripts/build-app.sh         # Native Mac architecture
./scripts/build-app.sh x64     # Intel Mac
```

Architecture-specific app bundles are written beneath
`target/<rust-target>/release/bundle/macos/FnScribe.app`. To notarize and create
an Intel installer, run `./scripts/notarize-app.sh x64`; the resulting file is
`target/release/bundle/dmg/FnScribe-x64.dmg`.

For development:

```sh
./scripts/download-model.sh
cargo tauri dev
```

The opt-in delivery regression harness exercises the production insertion path
against any focused control without adding application-specific runtime logic.
See [`scripts/DELIVERY_TESTING.md`](scripts/DELIVERY_TESTING.md) for the native,
terminal, browser, and Electron test matrix.

To use a different whisper.cpp GGML model during development, set
`FNSCRIBE_MODEL` to its absolute path.

## License

FnScribe is free software licensed under the
[GNU General Public License version 3](LICENSE). Copyright © 2026 Algorithmic
Research Group, Inc.
