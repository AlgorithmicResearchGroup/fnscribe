use crate::platform::macos;
use crate::state::{AppState, Phase};
use crate::tray;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_positioner::{Position, WindowExt};

pub const SNAPSHOT_EVENT: &str = "snapshot-changed";

pub fn set_status(app: &AppHandle, phase: Phase, message: impl Into<String>) {
    app.state::<AppState>().set_status(phase, message);
    publish(app);
}

pub fn publish(app: &AppHandle) {
    let state = app.state::<AppState>();
    let phase = state.status().phase;
    tray::update(app, phase);

    let snapshot = state.snapshot(macos::accessibility_trusted(), macos::microphone_trusted());
    let _ = app.emit(SNAPSHOT_EVENT, snapshot);

    let Some(window) = app.get_webview_window("flowbar") else {
        return;
    };
    if matches!(
        phase,
        Phase::Starting | Phase::Recording | Phase::Transcribing | Phase::Inserting
    ) {
        if !window.is_visible().unwrap_or(false) {
            let _ = window.move_window(Position::BottomCenter);
            let _ = window.show();
        }
    } else {
        let _ = window.hide();
    }
}
