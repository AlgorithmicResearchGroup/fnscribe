mod accuracy;
mod audio;
#[cfg(feature = "delivery-harness")]
mod delivery_harness;
mod dictation;
mod platform;
mod runtime_ui;
mod settings;
mod state;
mod transcriber;
mod tray;

use crate::accuracy::DictionaryEntry;
use crate::audio::{InputDeviceInfo, input_devices};
use crate::platform::macos;
use crate::state::{AppState, DictationMode, Phase, Snapshot};
use crate::transcriber::find_model;
use crate::tray::TRAY_ID;
use std::thread;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_positioner::{Position, WindowExt};

const MENU_OPEN: &str = "open";
const MENU_PASTE_LAST: &str = "paste-last";
const MENU_COPY_LAST: &str = "copy-last";
const MENU_QUIT: &str = "quit";

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.snapshot(macos::accessibility_trusted(), macos::microphone_trusted())
}

#[tauri::command]
fn get_microphones() -> Result<Vec<InputDeviceInfo>, String> {
    input_devices()
}

#[tauri::command]
fn request_keyboard_access(app: AppHandle) {
    macos::request_accessibility();
    runtime_ui::publish(&app);
}

#[tauri::command]
fn request_microphone(app: AppHandle) {
    macos::request_microphone();
    runtime_ui::publish(&app);
}

#[tauri::command]
fn begin_hotkey_capture(app: AppHandle, state: State<'_, AppState>) {
    state.begin_hotkey_capture();
    runtime_ui::publish(&app);
}

#[tauri::command]
fn cancel_hotkey_capture(app: AppHandle, state: State<'_, AppState>) {
    state.cancel_hotkey_capture();
    runtime_ui::publish(&app);
}

#[tauri::command]
fn set_hotkey(app: AppHandle, state: State<'_, AppState>, hotkey: String) -> Result<(), String> {
    state.cancel_hotkey_capture();
    change_hotkey(&app, &state, hotkey)
}

#[tauri::command]
fn set_microphone(
    app: AppHandle,
    state: State<'_, AppState>,
    microphone_id: Option<String>,
) -> Result<(), String> {
    if let Some(ref selected_id) = microphone_id {
        let available = input_devices()?;
        if !available.iter().any(|device| &device.id == selected_id) {
            return Err("That microphone is no longer available.".to_string());
        }
    }
    state.replace_microphone(microphone_id)?;
    runtime_ui::set_status(&app, Phase::Ready, "Microphone preference saved");
    Ok(())
}

#[tauri::command]
fn set_launch_at_login(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let previous = state.launch_at_login();
    let was_enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| format!("Could not read launch-at-login state: {error}"))?;
    if enabled != was_enabled {
        if enabled {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        }
        .map_err(|error| format!("Could not update launch at login: {error}"))?;
    }

    if enabled != previous
        && let Err(error) = state.replace_launch_at_login(enabled)
    {
        if was_enabled {
            let _ = app.autolaunch().enable();
        } else {
            let _ = app.autolaunch().disable();
        }
        return Err(error);
    }
    runtime_ui::publish(&app);
    Ok(())
}

#[tauri::command]
fn get_dictionary_entries(state: State<'_, AppState>) -> Vec<DictionaryEntry> {
    state.dictionary_entries()
}

#[tauri::command]
fn save_dictionary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    original_written_form: Option<String>,
    written_form: String,
    spoken_form: Option<String>,
) -> Result<Vec<DictionaryEntry>, String> {
    let entries = state.save_dictionary_entry(
        original_written_form.as_deref(),
        &written_form,
        spoken_form.as_deref(),
    )?;
    runtime_ui::set_status(&app, Phase::Ready, "Personal dictionary saved");
    Ok(entries)
}

#[tauri::command]
fn delete_dictionary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    written_form: String,
) -> Result<Vec<DictionaryEntry>, String> {
    let entries = state.delete_dictionary_entry(&written_form)?;
    runtime_ui::set_status(&app, Phase::Ready, "Dictionary entry removed");
    Ok(entries)
}

#[tauri::command]
fn set_smart_cleanup(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.replace_smart_cleanup(enabled)?;
    runtime_ui::set_status(
        &app,
        Phase::Ready,
        if enabled {
            "Smart cleanup enabled"
        } else {
            "Smart cleanup disabled"
        },
    );
    Ok(())
}

#[tauri::command]
fn toggle_hands_free(app: AppHandle) {
    dictation::toggle_hands_free(&app);
}

#[tauri::command]
fn stop_dictation(app: AppHandle) {
    dictation::stop(&app);
}

#[tauri::command]
fn cancel_dictation(app: AppHandle) {
    dictation::cancel(&app);
}

#[tauri::command]
fn copy_last_transcript(app: AppHandle) -> Result<(), String> {
    dictation::copy_last(&app)
}

#[tauri::command]
fn copy_original_transcript(app: AppHandle) -> Result<(), String> {
    dictation::copy_original(&app)
}

#[tauri::command]
fn paste_last_transcript(app: AppHandle) -> Result<(), String> {
    dictation::paste_last(&app)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn change_hotkey(app: &AppHandle, state: &AppState, hotkey: String) -> Result<(), String> {
    let hotkey = if hotkey.eq_ignore_ascii_case("fn") {
        "Fn".to_string()
    } else {
        hotkey
    };
    let old_hotkey = state.hotkey();
    if hotkey == old_hotkey {
        if !state.hotkey_registered() {
            if hotkey != "Fn" {
                app.global_shortcut()
                    .register(hotkey.as_str())
                    .map_err(|error| format!("That shortcut is unavailable: {error}"))?;
            }
            state.mark_hotkey_registered(true);
            runtime_ui::set_status(app, Phase::Ready, "Shortcut restored — ready");
        } else {
            runtime_ui::publish(app);
        }
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
        let restored = old_is_fn
            || (old_was_registered && app.global_shortcut().register(old_hotkey.as_str()).is_ok());
        state.mark_hotkey_registered(restored);
        return Err(error);
    }
    state.mark_hotkey_registered(true);
    if state.transcriber.is_ready() {
        runtime_ui::set_status(app, Phase::Ready, "Ready — hold the shortcut to talk");
    } else {
        runtime_ui::publish(app);
    }
    Ok(())
}

fn hotkey_event(app: &AppHandle, event_state: ShortcutState) {
    let state = app.state::<AppState>();
    if state.is_capturing_hotkey() || state.hotkey() == "Fn" {
        return;
    }
    match event_state {
        ShortcutState::Pressed => dictation::begin(app, DictationMode::PushToTalk),
        ShortcutState::Released => dictation::release_push_to_talk(app),
    }
}

fn fn_key_event(app: &AppHandle, event: macos::FnEvent) {
    let state = app.state::<AppState>();
    match event {
        macos::FnEvent::Ready if state.hotkey() == "Fn" && state.transcriber.is_ready() => {
            runtime_ui::set_status(app, Phase::Ready, "Ready — hold fn to talk");
        }
        macos::FnEvent::Pressed if state.take_hotkey_capture() => {
            if let Err(error) = change_hotkey(app, &state, "Fn".to_string()) {
                runtime_ui::set_status(app, Phase::Error, error);
            }
        }
        macos::FnEvent::Pressed if state.hotkey() == "Fn" => {
            dictation::begin(app, DictationMode::PushToTalk);
        }
        macos::FnEvent::Released if state.hotkey() == "Fn" => {
            dictation::release_push_to_talk(app);
        }
        macos::FnEvent::HandsFreeToggle => dictation::toggle_hands_free(app),
        macos::FnEvent::Cancel => dictation::cancel(app),
        macos::FnEvent::Unavailable if state.hotkey() == "Fn" => runtime_ui::set_status(
            app,
            Phase::Error,
            "Could not listen for fn — re-grant Keyboard access",
        ),
        _ => {}
    }
}

fn show_or_hide_settings(app: &AppHandle) {
    let Some(window) = app.get_webview_window("settings") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    remember_frontmost_target(app);
    let _ = window.move_window_constrained(Position::TrayCenter);
    let _ = window.show();
    let _ = window.set_focus();
    runtime_ui::publish(app);
}

fn show_settings(app: &AppHandle) {
    let Some(window) = app.get_webview_window("settings") else {
        return;
    };
    remember_frontmost_target(app);
    let _ = window.move_window_constrained(Position::TrayCenter);
    let _ = window.show();
    let _ = window.set_focus();
    runtime_ui::publish(app);
}

fn remember_frontmost_target(app: &AppHandle) {
    if let Some(pid) = macos::frontmost_application_pid()
        && pid != std::process::id() as i32
    {
        app.state::<AppState>().remember_target_pid(pid);
    }
}

fn load_model(app: AppHandle) {
    let resource_dir = app.path().resource_dir().ok();
    let result = find_model(resource_dir.as_deref())
        .and_then(|path| app.state::<AppState>().transcriber.load(&path));

    match result {
        Ok(()) if app.state::<AppState>().hotkey_registered() => {
            runtime_ui::set_status(&app, Phase::Ready, "Ready — hold the shortcut to talk");
        }
        Ok(()) => runtime_ui::publish(&app),
        Err(error) => {
            app.state::<AppState>()
                .transcriber
                .set_load_error(error.clone());
            runtime_ui::set_status(&app, Phase::Error, error);
        }
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Open FnScribe", true, None::<&str>)?;
    let paste = MenuItem::with_id(
        app,
        MENU_PASTE_LAST,
        "Paste Last Transcript",
        true,
        None::<&str>,
    )?;
    let copy = MenuItem::with_id(
        app,
        MENU_COPY_LAST,
        "Copy Last Transcript",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit FnScribe", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &paste, &copy, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray::icon(Phase::Loading))
        .icon_as_template(true)
        .tooltip("FnScribe — loading")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => show_settings(app),
            MENU_PASTE_LAST => {
                if let Err(error) = dictation::paste_last(app) {
                    runtime_ui::set_status(app, Phase::Error, error);
                }
            }
            MENU_COPY_LAST => {
                if let Err(error) = dictation::copy_last(app) {
                    runtime_ui::set_status(app, Phase::Error, error);
                }
            }
            MENU_QUIT => app.exit(0),
            _ => {}
        })
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
    Ok(())
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(not(feature = "delivery-harness"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_settings(app);
    }));

    builder
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_positioner::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| hotkey_event(app, event.state))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_microphones,
            request_keyboard_access,
            request_microphone,
            begin_hotkey_capture,
            cancel_hotkey_capture,
            set_hotkey,
            set_microphone,
            set_launch_at_login,
            get_dictionary_entries,
            save_dictionary_entry,
            delete_dictionary_entry,
            set_smart_cleanup,
            toggle_hands_free,
            stop_dictation,
            cancel_dictation,
            copy_last_transcript,
            copy_original_transcript,
            paste_last_transcript,
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
            let launch_at_login = saved_settings.launch_at_login;
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

            let dictation_active = app.state::<AppState>().dictation_active_flag();
            let fn_handle = app.handle().clone();
            macos::start_fn_monitor(dictation_active, move |event| {
                fn_key_event(&fn_handle, event)
            });

            if launch_at_login {
                let _ = app.autolaunch().enable();
            } else {
                let _ = app.autolaunch().disable();
            }

            build_tray(app)?;
            runtime_ui::publish(app.handle());

            let handle = app.handle().clone();
            thread::spawn(move || load_model(handle));
            #[cfg(feature = "delivery-harness")]
            delivery_harness::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                match event {
                    WindowEvent::Focused(false) => {
                        let _ = window.hide();
                    }
                    WindowEvent::Focused(true) => runtime_ui::publish(window.app_handle()),
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running FnScribe");
}
