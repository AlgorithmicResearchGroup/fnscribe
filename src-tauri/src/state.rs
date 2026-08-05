use crate::audio::AudioRecorder;
use crate::settings::{self, AppSettings};
use crate::transcriber::Transcriber;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Ready,
    Recording,
    Transcribing,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Recording => "recording",
            Self::Transcribing => "transcribing",
            Self::Error => "error",
        }
    }
}

#[derive(Clone)]
pub struct RuntimeStatus {
    pub phase: Phase,
    pub message: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub phase: &'static str,
    pub message: String,
    pub hotkey: String,
    pub capturing_hotkey: bool,
    pub accessibility_trusted: bool,
    pub microphone_trusted: bool,
}

pub struct AppState {
    pub transcriber: Transcriber,
    pub recorder: Mutex<Option<AudioRecorder>>,
    status: Mutex<RuntimeStatus>,
    hotkey: Mutex<String>,
    settings_path: PathBuf,
    capturing_hotkey: AtomicBool,
    fn_monitor_ready: AtomicBool,
    hotkey_registered: AtomicBool,
    target_pid: AtomicI32,
}

impl AppState {
    pub fn new(settings_path: PathBuf, app_settings: AppSettings) -> Self {
        Self {
            transcriber: Transcriber::new(),
            recorder: Mutex::new(None),
            status: Mutex::new(RuntimeStatus {
                phase: Phase::Loading,
                message: "Loading local model…".to_string(),
            }),
            hotkey: Mutex::new(app_settings.hotkey),
            settings_path,
            capturing_hotkey: AtomicBool::new(false),
            fn_monitor_ready: AtomicBool::new(false),
            hotkey_registered: AtomicBool::new(false),
            target_pid: AtomicI32::new(0),
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&self, phase: Phase, message: impl Into<String>) {
        *self.status.lock().unwrap() = RuntimeStatus {
            phase,
            message: message.into(),
        };
    }

    pub fn hotkey(&self) -> String {
        self.hotkey.lock().unwrap().clone()
    }

    pub fn replace_hotkey(&self, hotkey: String) -> Result<(), String> {
        settings::save(
            &self.settings_path,
            &AppSettings {
                hotkey: hotkey.clone(),
            },
        )?;
        *self.hotkey.lock().unwrap() = hotkey;
        Ok(())
    }

    pub fn hotkey_registered(&self) -> bool {
        self.hotkey_registered.load(Ordering::Relaxed)
    }

    pub fn mark_hotkey_registered(&self, registered: bool) {
        self.hotkey_registered.store(registered, Ordering::Relaxed);
    }

    pub fn begin_hotkey_capture(&self) {
        self.capturing_hotkey.store(true, Ordering::Relaxed);
    }

    pub fn cancel_hotkey_capture(&self) {
        self.capturing_hotkey.store(false, Ordering::Relaxed);
    }

    pub fn take_hotkey_capture(&self) -> bool {
        self.capturing_hotkey.swap(false, Ordering::Relaxed)
    }

    pub fn is_capturing_hotkey(&self) -> bool {
        self.capturing_hotkey.load(Ordering::Relaxed)
    }

    pub fn mark_fn_monitor_ready(&self) {
        self.fn_monitor_ready.store(true, Ordering::Relaxed);
    }

    pub fn remember_target_pid(&self, pid: i32) {
        self.target_pid.store(pid, Ordering::Relaxed);
    }

    pub fn target_pid(&self) -> Option<i32> {
        match self.target_pid.load(Ordering::Relaxed) {
            0 => None,
            pid => Some(pid),
        }
    }

    pub fn snapshot(&self, accessibility_trusted: bool, microphone_trusted: bool) -> Snapshot {
        let status = self.status();
        Snapshot {
            phase: status.phase.as_str(),
            message: status.message,
            hotkey: self.hotkey(),
            capturing_hotkey: self.is_capturing_hotkey(),
            accessibility_trusted,
            microphone_trusted,
        }
    }
}
