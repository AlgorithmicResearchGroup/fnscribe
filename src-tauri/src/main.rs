mod audio;
mod platform;
mod settings;
mod state;
mod transcriber;
mod tray;

use crate::audio::AudioRecorder;
use crate::platform::macos;
use crate::state::{AppState, Phase, Snapshot};
use crate::transcriber::find_model;
use crate::tray::TRAY_ID;
use std::thread;
use std::time::Duration;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_positioner::{Position, WindowExt};

const MIN_RECORDING_SECONDS: f32 = 0.30;
const MIN_AUDIO_RMS: f32 = 0.0015;

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.snapshot(macos::accessibility_trusted(), macos::microphone_trusted())
}

#[tauri::command]
fn request_keyboard_access() {
    macos::request_accessibility();
}

#[tauri::command]
fn request_microphone() {
    macos::request_microphone();
}

#[tauri::command]
fn begin_hotkey_capture(state: State<'_, AppState>) {
    state.begin_hotkey_capture();
}

#[tauri::command]
fn cancel_hotkey_capture(state: State<'_, AppState>) {
    state.cancel_hotkey_capture();
}

#[tauri::command]
fn set_hotkey(app: AppHandle, state: State<'_, AppState>, hotkey: String) -> Result<(), String> {
    state.cancel_hotkey_capture();
    change_hotkey(&app, &state, hotkey)
}

fn change_hotkey(app: &AppHandle, state: &AppState, hotkey: String) -> Result<(), String> {
    let hotkey = if hotkey.eq_ignore_ascii_case("fn") {
        "Fn".to_string()
    } else {
        hotkey
    };
    let old_hotkey = state.hotkey();
    if hotkey == old_hotkey {
        return Ok(());
    }
    let old_is_fn = old_hotkey == "Fn";
    let new_is_fn = hotkey == "Fn";
    let old_was_registered = state.hotkey_registered();

    if !new_is_fn {
        app.global_shortcut()
            .register(hotkey.as_str())
            .map_err(|error| format!("That shortcut is unavailable: {error}"))?;
    }

    if !old_is_fn && old_was_registered {
        if let Err(error) = app.global_shortcut().unregister(old_hotkey.as_str()) {
            if !new_is_fn {
                let _ = app.global_shortcut().unregister(hotkey.as_str());
            }
            return Err(format!("Could not replace the old shortcut: {error}"));
        }
    }

    if let Err(error) = state.replace_hotkey(hotkey.clone()) {
        if !new_is_fn {
            let _ = app.global_shortcut().unregister(hotkey.as_str());
        }
        if !old_is_fn && old_was_registered {
            let _ = app.global_shortcut().register(old_hotkey.as_str());
        }
        return Err(error);
    }
    state.mark_hotkey_registered(true);
    if state.transcriber.is_ready() {
        set_status(app, Phase::Ready, "Ready — hold the shortcut to talk");
    }

    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn set_status(app: &AppHandle, phase: Phase, message: impl Into<String>) {
    app.state::<AppState>().set_status(phase, message);
    tray::update(app, phase);
}

fn hotkey_event(app: &AppHandle, event_state: ShortcutState) {
    let state = app.state::<AppState>();
    if state.is_capturing_hotkey() || state.hotkey() == "Fn" {
        return;
    }
    match event_state {
        ShortcutState::Pressed => begin_recording(app),
        ShortcutState::Released => finish_recording(app),
    }
}

fn fn_key_event(app: &AppHandle, event: macos::FnEvent) {
    let state = app.state::<AppState>();
    match event {
        macos::FnEvent::Ready => {
            state.mark_fn_monitor_ready();
            if state.hotkey() == "Fn" && state.transcriber.is_ready() {
                set_status(app, Phase::Ready, "Ready — hold fn to talk");
            }
        }
        macos::FnEvent::Pressed if state.take_hotkey_capture() => {
            if let Err(error) = change_hotkey(app, &state, "Fn".to_string()) {
                set_status(app, Phase::Error, error);
            }
        }
        macos::FnEvent::Pressed if state.hotkey() == "Fn" => begin_recording(app),
        macos::FnEvent::Released if state.hotkey() == "Fn" => finish_recording(app),
        macos::FnEvent::Unavailable if state.hotkey() == "Fn" => set_status(
            app,
            Phase::Error,
            "Could not listen for fn — re-grant Keyboard access",
        ),
        _ => {}
    }
}

fn begin_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    let phase = state.status().phase;
    if matches!(
        phase,
        Phase::Loading | Phase::Recording | Phase::Transcribing
    ) {
        return;
    }

    if !state.transcriber.is_ready() {
        let message = state
            .transcriber
            .load_error()
            .unwrap_or_else(|| "The local transcription model is not ready.".to_string());
        set_status(app, Phase::Error, message);
        return;
    }

    if !macos::accessibility_trusted() {
        macos::request_accessibility();
        set_status(
            app,
            Phase::Error,
            "Grant Accessibility permission, then try again.",
        );
        return;
    }

    if !macos::microphone_trusted() {
        macos::request_microphone();
        set_status(
            app,
            Phase::Error,
            "Grant Microphone permission, then try again.",
        );
        return;
    }

    if let Some(pid) = macos::frontmost_application_pid()
        && pid != std::process::id() as i32
    {
        state.remember_target_pid(pid);
    }

    match AudioRecorder::start() {
        Ok(recorder) => {
            *state.recorder.lock().unwrap() = Some(recorder);
            set_status(app, Phase::Recording, "Listening… release to transcribe");
        }
        Err(error) => {
            set_status(app, Phase::Error, error);
        }
    }
}

fn finish_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.status().phase != Phase::Recording {
        return;
    }
    let Some(recorder) = state.recorder.lock().unwrap().take() else {
        return;
    };
    set_status(app, Phase::Transcribing, "Transcribing locally…");

    let app = app.clone();
    thread::spawn(move || {
        let captured = match recorder.finish() {
            Ok(captured) => captured,
            Err(error) => {
                set_status(&app, Phase::Error, error);
                return;
            }
        };

        if captured.duration_seconds < MIN_RECORDING_SECONDS {
            set_status(
                &app,
                Phase::Error,
                "Recording was too short — hold fn while speaking",
            );
            return;
        }
        if captured.rms < MIN_AUDIO_RMS {
            set_status(&app, Phase::Error, "No microphone audio detected");
            return;
        }

        let transcript = match app
            .state::<AppState>()
            .transcriber
            .transcribe(&captured.samples)
        {
            Ok(transcript) => transcript,
            Err(error) => {
                set_status(&app, Phase::Error, error);
                return;
            }
        };

        if transcript.is_empty() {
            set_status(&app, Phase::Ready, "No speech detected");
            return;
        }

        if let Some(pid) = app.state::<AppState>().target_pid() {
            let _ = macos::activate_application(pid);
        }
        // Give macOS time to restore the target application's focused field
        // and finish dispatching the physical shortcut's key-up event.
        thread::sleep(Duration::from_millis(160));
        let text = format!("{} ", transcript.trim());
        match macos::insert_text(&text) {
            Ok(()) => set_status(&app, Phase::Ready, "Ready — hold the shortcut to talk"),
            Err(error) => set_status(&app, Phase::Error, error),
        }
    });
}

fn show_or_hide_settings(app: &AppHandle) {
    let Some(window) = app.get_webview_window("settings") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    if let Some(pid) = macos::frontmost_application_pid()
        && pid != std::process::id() as i32
    {
        app.state::<AppState>().remember_target_pid(pid);
    }
    let _ = window.move_window_constrained(Position::TrayCenter);
    let _ = window.show();
    let _ = window.set_focus();
}

fn load_model(app: AppHandle) {
    let resource_dir = app.path().resource_dir().ok();
    let result = find_model(resource_dir.as_deref())
        .and_then(|path| app.state::<AppState>().transcriber.load(&path));

    match result {
        Ok(()) if app.state::<AppState>().hotkey_registered() => {
            set_status(&app, Phase::Ready, "Ready — hold the shortcut to talk");
        }
        Ok(()) => tray::update(&app, Phase::Error),
        Err(error) => {
            app.state::<AppState>()
                .transcriber
                .set_load_error(error.clone());
            set_status(&app, Phase::Error, error);
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| hotkey_event(app, event.state))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            request_keyboard_access,
            request_microphone,
            begin_hotkey_capture,
            cancel_hotkey_capture,
            set_hotkey,
            quit_app
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|error| format!("Could not locate the settings directory: {error}"))?;
            let settings_path = settings::path_in(config_dir);
            let saved_settings = settings::load(&settings_path);
            app.manage(AppState::new(settings_path, saved_settings));
            macos::prompt_for_microphone_if_needed();

            let configured_hotkey = app.state::<AppState>().hotkey();
            if configured_hotkey == "Fn" {
                app.state::<AppState>().mark_hotkey_registered(true);
            } else if let Err(error) = app.global_shortcut().register(configured_hotkey.as_str()) {
                app.state::<AppState>().set_status(
                    Phase::Error,
                    format!("Shortcut unavailable. Choose another: {error}"),
                );
            } else {
                app.state::<AppState>().mark_hotkey_registered(true);
            }

            let fn_handle = app.handle().clone();
            macos::start_fn_monitor(move |event| fn_key_event(&fn_handle, event));

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray::icon(Phase::Loading))
                .icon_as_template(true)
                .tooltip("FnScribe — loading")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_or_hide_settings(tray.app_handle());
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            thread::spawn(move || load_model(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "settings" && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running FnScribe");
}
