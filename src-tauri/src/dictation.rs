use crate::accuracy::{process_transcript, vocabulary_prompt};
use crate::audio::{AudioRecorder, MAX_RECORDING_SECONDS};
use crate::platform::macos;
use crate::runtime_ui;
use crate::state::{AppState, DictationMode, Phase, SessionWork};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const MIN_RECORDING_SECONDS: f32 = 0.30;
const MIN_AUDIO_RMS: f32 = 0.0015;
const DEADLINE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const TARGET_FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const TARGET_FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(100);
const TARGET_FOCUS_ATTEMPTS: usize = 20;

pub fn begin(app: &AppHandle, mode: DictationMode) {
    let state = app.state::<AppState>();
    if !state.transcriber.is_ready() {
        let message = state
            .transcriber
            .load_error()
            .unwrap_or_else(|| "The local transcription model is not ready.".to_string());
        runtime_ui::set_status(app, Phase::Error, message);
        return;
    }

    if !macos::accessibility_trusted() {
        macos::request_accessibility();
        runtime_ui::set_status(
            app,
            Phase::Error,
            "Grant Accessibility permission, then try again.",
        );
        return;
    }

    if !macos::microphone_trusted() {
        macos::request_microphone();
        runtime_ui::set_status(
            app,
            Phase::Error,
            "Grant Microphone permission, then try again.",
        );
        return;
    }

    let target_pid = macos::frontmost_application_pid()
        .filter(|pid| *pid != std::process::id() as i32)
        .or_else(|| state.last_target_pid());
    let ticket = match state.reserve_session(mode, target_pid) {
        Ok(ticket) => ticket,
        Err(_) => return,
    };
    runtime_ui::publish(app);

    let preferred_microphone = state.microphone_id();
    let recorder = match AudioRecorder::start(preferred_microphone.as_deref()) {
        Ok(recorder) => recorder,
        Err(error) => {
            if state.fail_session(ticket.id, error) {
                runtime_ui::publish(app);
            }
            return;
        }
    };

    let microphone_name = recorder.microphone().name.clone();
    let message = if recorder.used_fallback() {
        format!("Listening on {microphone_name} — selected microphone unavailable")
    } else if mode == DictationMode::HandsFree {
        format!("Listening hands-free on {microphone_name}…")
    } else {
        format!("Listening on {microphone_name}… release to transcribe")
    };
    if state.attach_recorder(ticket.id, recorder, message).is_err() {
        return;
    }
    runtime_ui::publish(app);

    let app = app.clone();
    thread::spawn(move || {
        let polls = MAX_RECORDING_SECONDS * 1_000 / DEADLINE_POLL_INTERVAL.as_millis() as usize;
        for _ in 0..polls {
            if ticket.cancel.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(DEADLINE_POLL_INTERVAL);
            if !app.state::<AppState>().is_recording_session(ticket.id) {
                return;
            }
        }
        finish_session(&app, Some(ticket.id));
    });
}

pub fn release_push_to_talk(app: &AppHandle) {
    if app.state::<AppState>().recording_mode() == Some(DictationMode::PushToTalk) {
        finish_session(app, None);
    }
}

pub fn toggle_hands_free(app: &AppHandle) {
    match app.state::<AppState>().recording_mode() {
        Some(DictationMode::PushToTalk) => {
            if app.state::<AppState>().lock_hands_free() {
                runtime_ui::publish(app);
            }
        }
        Some(DictationMode::HandsFree) => finish_session(app, None),
        None if app.state::<AppState>().status().phase == Phase::Ready
            || app.state::<AppState>().status().phase == Phase::Error =>
        {
            begin(app, DictationMode::HandsFree);
        }
        None => {}
    }
}

pub fn stop(app: &AppHandle) {
    finish_session(app, None);
}

pub fn cancel(app: &AppHandle) {
    let target_pid = app.state::<AppState>().active_target_pid();
    if app.state::<AppState>().cancel_session() {
        if let Some(pid) = target_pid {
            let _ = macos::activate_application(pid);
        }
        runtime_ui::publish(app);
    }
}

fn finish_session(app: &AppHandle, expected_session_id: Option<u64>) {
    let Some(work) = app.state::<AppState>().take_recording(expected_session_id) else {
        return;
    };
    runtime_ui::publish(app);

    let app = app.clone();
    thread::spawn(move || transcribe_and_deliver(app, work));
}

fn transcribe_and_deliver(app: AppHandle, work: SessionWork) {
    let SessionWork {
        id,
        target_pid,
        cancel,
        recorder,
    } = work;
    let captured = match recorder.finish() {
        Ok(captured) => captured,
        Err(error) => {
            fail_if_current(&app, id, error);
            return;
        }
    };
    if cancelled(&cancel) {
        return;
    }

    if captured.duration_seconds < MIN_RECORDING_SECONDS {
        fail_if_current(
            &app,
            id,
            "Recording was too short — hold the shortcut while speaking",
        );
        return;
    }
    if captured.rms < MIN_AUDIO_RMS {
        fail_if_current(&app, id, "No microphone audio detected");
        return;
    }

    let (dictionary, smart_cleanup) = app.state::<AppState>().accuracy_settings();
    let prompt = vocabulary_prompt(&dictionary);
    let transcript = match app.state::<AppState>().transcriber.transcribe(
        &captured.samples,
        &cancel,
        prompt.as_deref(),
    ) {
        Ok(transcript) => transcript,
        Err(error) => {
            fail_if_current(&app, id, error);
            return;
        }
    };
    if cancelled(&cancel) {
        return;
    }
    let original_transcript = transcript.trim().to_string();
    let transcript = process_transcript(&original_transcript, &dictionary, smart_cleanup);
    if transcript.is_empty() {
        complete_if_current(&app, id, "No speech detected");
        return;
    }

    let text = format!("{} ", transcript.trim());
    let original = (original_transcript != transcript).then_some(original_transcript);
    if !app
        .state::<AppState>()
        .stage_transcript(id, text.clone(), original)
    {
        return;
    }
    runtime_ui::publish(&app);

    let Some(target_pid) = target_pid else {
        fail_if_current(
            &app,
            id,
            "Could not identify the target app — transcript kept for recovery",
        );
        return;
    };
    if focus_target_application(target_pid).is_err() {
        fail_if_current(
            &app,
            id,
            "Could not return to the target app — transcript kept for recovery",
        );
        return;
    }
    if cancelled(&cancel) || !app.state::<AppState>().is_current_session(id) {
        return;
    }

    match macos::insert_text(&app, target_pid, &text) {
        Ok(()) => complete_if_current(&app, id, "Ready — hold the shortcut to talk"),
        Err(error) => fail_if_current(&app, id, error),
    }
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Acquire)
}

fn focus_target_application(pid: i32) -> Result<(), String> {
    if macos::frontmost_application_pid() != Some(pid) {
        if !macos::activate_application(pid) {
            return Err("The target app is no longer available.".to_string());
        }
        for _ in 0..TARGET_FOCUS_ATTEMPTS {
            thread::sleep(TARGET_FOCUS_POLL_INTERVAL);
            if macos::frontmost_application_pid() == Some(pid) {
                break;
            }
        }
    }
    thread::sleep(TARGET_FOCUS_SETTLE_DELAY);
    if macos::frontmost_application_pid() == Some(pid) {
        Ok(())
    } else {
        Err("The target app did not accept focus.".to_string())
    }
}

fn complete_if_current(app: &AppHandle, session_id: u64, message: impl Into<String>) {
    if app
        .state::<AppState>()
        .complete_session(session_id, message)
    {
        runtime_ui::publish(app);
    }
}

fn fail_if_current(app: &AppHandle, session_id: u64, message: impl Into<String>) {
    if app.state::<AppState>().fail_session(session_id, message) {
        runtime_ui::publish(app);
    }
}

pub fn copy_last(app: &AppHandle) -> Result<(), String> {
    let text = app
        .state::<AppState>()
        .last_transcript()
        .ok_or_else(|| "There is no transcript to copy yet.".to_string())?;
    macos::copy_text(app, &text)?;
    runtime_ui::set_status(app, Phase::Ready, "Last transcript copied");
    Ok(())
}

pub fn copy_original(app: &AppHandle) -> Result<(), String> {
    let text = app
        .state::<AppState>()
        .last_original_transcript()
        .ok_or_else(|| "The last transcript was not changed by smart cleanup.".to_string())?;
    macos::copy_text(app, &text)?;
    runtime_ui::set_status(app, Phase::Ready, "Original transcript copied");
    Ok(())
}

pub fn paste_last(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.has_active_session() {
        return Err(
            "Finish or cancel the active dictation before pasting recovery text.".to_string(),
        );
    }
    let text = state
        .last_transcript()
        .ok_or_else(|| "There is no transcript to paste yet.".to_string())?;
    if !macos::accessibility_trusted() {
        return Err("Accessibility permission is required to paste.".to_string());
    }
    let target_pid = macos::frontmost_application_pid()
        .filter(|pid| *pid != std::process::id() as i32)
        .or_else(|| state.last_target_pid())
        .ok_or_else(|| "Open the target app before pasting the transcript.".to_string())?;
    focus_target_application(target_pid)?;
    macos::insert_text(app, target_pid, &text)?;
    runtime_ui::set_status(app, Phase::Ready, "Last transcript pasted");
    Ok(())
}
