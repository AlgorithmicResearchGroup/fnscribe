use crate::accuracy::{
    DictionaryEntry, MAX_DICTIONARY_ENTRIES, prepare_dictionary_entry, validate_dictionary,
};
use crate::audio::AudioRecorder;
use crate::settings::{self, AppSettings};
use crate::transcriber::Transcriber;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Loading,
    Ready,
    Starting,
    Recording,
    Transcribing,
    Inserting,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Starting => "starting",
            Self::Recording => "recording",
            Self::Transcribing => "transcribing",
            Self::Inserting => "inserting",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationMode {
    PushToTalk,
    HandsFree,
}

impl DictationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PushToTalk => "push_to_talk",
            Self::HandsFree => "hands_free",
        }
    }
}

#[derive(Clone)]
pub struct RuntimeStatus {
    pub phase: Phase,
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub phase: &'static str,
    pub message: String,
    pub recording_mode: Option<&'static str>,
    pub hotkey: String,
    pub capturing_hotkey: bool,
    pub accessibility_trusted: bool,
    pub microphone_trusted: bool,
    pub microphone_id: Option<String>,
    pub launch_at_login: bool,
    pub smart_cleanup: bool,
    pub dictionary_count: usize,
    pub has_last_transcript: bool,
    pub has_original_transcript: bool,
}

pub struct SessionTicket {
    pub id: u64,
    pub cancel: Arc<AtomicBool>,
}

pub struct SessionWork {
    pub id: u64,
    pub target_pid: Option<i32>,
    pub cancel: Arc<AtomicBool>,
    pub recorder: AudioRecorder,
}

struct ActiveSession {
    id: u64,
    mode: DictationMode,
    target_pid: Option<i32>,
    cancel: Arc<AtomicBool>,
    recorder: Option<AudioRecorder>,
}

struct RuntimeState {
    status: RuntimeStatus,
    active_session: Option<ActiveSession>,
    last_transcript: Option<String>,
    last_original_transcript: Option<String>,
    last_target_pid: Option<i32>,
    next_session_id: u64,
}

pub struct AppState {
    pub transcriber: Transcriber,
    runtime: Mutex<RuntimeState>,
    settings: Mutex<AppSettings>,
    settings_path: PathBuf,
    capturing_hotkey: AtomicBool,
    hotkey_registered: AtomicBool,
    dictation_active: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(settings_path: PathBuf, app_settings: AppSettings) -> Self {
        Self {
            transcriber: Transcriber::new(),
            runtime: Mutex::new(RuntimeState {
                status: RuntimeStatus {
                    phase: Phase::Loading,
                    message: "Loading local model…".to_string(),
                },
                active_session: None,
                last_transcript: None,
                last_original_transcript: None,
                last_target_pid: None,
                next_session_id: 1,
            }),
            settings: Mutex::new(app_settings),
            settings_path,
            capturing_hotkey: AtomicBool::new(false),
            hotkey_registered: AtomicBool::new(false),
            dictation_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        lock(&self.runtime).status.clone()
    }

    pub fn set_status(&self, phase: Phase, message: impl Into<String>) {
        let mut runtime = lock(&self.runtime);
        // Preference, permission, and tray actions can arrive while a recording
        // worker owns the state machine. They may report UI feedback, but must
        // never make an active session appear idle or strand its recorder.
        if runtime.active_session.is_none() {
            runtime.status = RuntimeStatus {
                phase,
                message: message.into(),
            };
        }
    }

    pub fn reserve_session(
        &self,
        mode: DictationMode,
        target_pid: Option<i32>,
    ) -> Result<SessionTicket, String> {
        let mut runtime = lock(&self.runtime);
        if runtime.active_session.is_some() {
            return Err("A dictation is already in progress.".to_string());
        }

        let id = runtime.next_session_id;
        runtime.next_session_id = runtime.next_session_id.wrapping_add(1).max(1);
        let cancel = Arc::new(AtomicBool::new(false));
        runtime.active_session = Some(ActiveSession {
            id,
            mode,
            target_pid,
            cancel: cancel.clone(),
            recorder: None,
        });
        runtime.status = RuntimeStatus {
            phase: Phase::Starting,
            message: "Starting microphone…".to_string(),
        };
        self.dictation_active.store(true, Ordering::Release);
        Ok(SessionTicket { id, cancel })
    }

    pub fn attach_recorder(
        &self,
        session_id: u64,
        recorder: AudioRecorder,
        message: impl Into<String>,
    ) -> Result<(), AudioRecorder> {
        let mut runtime = lock(&self.runtime);
        let Some(session) = runtime.active_session.as_mut() else {
            return Err(recorder);
        };
        if session.id != session_id || session.cancel.load(Ordering::Acquire) {
            return Err(recorder);
        }
        session.recorder = Some(recorder);
        runtime.status = RuntimeStatus {
            phase: Phase::Recording,
            message: message.into(),
        };
        Ok(())
    }

    pub fn recording_mode(&self) -> Option<DictationMode> {
        let runtime = lock(&self.runtime);
        runtime
            .active_session
            .as_ref()
            .and_then(|session| (runtime.status.phase == Phase::Recording).then_some(session.mode))
    }

    pub fn lock_hands_free(&self) -> bool {
        let mut runtime = lock(&self.runtime);
        if runtime.status.phase != Phase::Recording {
            return false;
        }
        let Some(session) = runtime.active_session.as_mut() else {
            return false;
        };
        session.mode = DictationMode::HandsFree;
        runtime.status = RuntimeStatus {
            phase: Phase::Recording,
            message: "Listening hands-free… press fn + space to stop".to_string(),
        };
        true
    }

    pub fn take_recording(&self, expected_session_id: Option<u64>) -> Option<SessionWork> {
        let mut runtime = lock(&self.runtime);
        if runtime.status.phase != Phase::Recording {
            return None;
        }
        let session = runtime.active_session.as_mut()?;
        if expected_session_id.is_some_and(|expected| expected != session.id) {
            return None;
        }
        let recorder = session.recorder.take()?;
        let work = SessionWork {
            id: session.id,
            target_pid: session.target_pid,
            cancel: session.cancel.clone(),
            recorder,
        };
        runtime.status = RuntimeStatus {
            phase: Phase::Transcribing,
            message: "Transcribing locally…".to_string(),
        };
        Some(work)
    }

    pub fn cancel_session(&self) -> bool {
        let mut runtime = lock(&self.runtime);
        let Some(session) = runtime.active_session.take() else {
            return false;
        };
        session.cancel.store(true, Ordering::Release);
        runtime.status = RuntimeStatus {
            phase: Phase::Ready,
            message: "Dictation cancelled — ready".to_string(),
        };
        self.dictation_active.store(false, Ordering::Release);
        true
    }

    pub fn is_current_session(&self, session_id: u64) -> bool {
        lock(&self.runtime)
            .active_session
            .as_ref()
            .is_some_and(|session| {
                session.id == session_id && !session.cancel.load(Ordering::Acquire)
            })
    }

    pub fn has_active_session(&self) -> bool {
        lock(&self.runtime).active_session.is_some()
    }

    pub fn active_target_pid(&self) -> Option<i32> {
        lock(&self.runtime)
            .active_session
            .as_ref()
            .and_then(|session| session.target_pid)
    }

    pub fn is_recording_session(&self, session_id: u64) -> bool {
        let runtime = lock(&self.runtime);
        runtime.status.phase == Phase::Recording
            && runtime
                .active_session
                .as_ref()
                .is_some_and(|session| session.id == session_id)
    }

    pub fn stage_transcript(
        &self,
        session_id: u64,
        transcript: String,
        original_transcript: Option<String>,
    ) -> bool {
        let mut runtime = lock(&self.runtime);
        let is_current = runtime.active_session.as_ref().is_some_and(|session| {
            session.id == session_id && !session.cancel.load(Ordering::Acquire)
        });
        if !is_current {
            return false;
        }
        runtime.last_transcript = Some(transcript);
        runtime.last_original_transcript = original_transcript;
        runtime.status = RuntimeStatus {
            phase: Phase::Inserting,
            message: "Inserting transcript…".to_string(),
        };
        true
    }

    pub fn complete_session(&self, session_id: u64, message: impl Into<String>) -> bool {
        self.end_session(session_id, Phase::Ready, message)
    }

    pub fn fail_session(&self, session_id: u64, message: impl Into<String>) -> bool {
        self.end_session(session_id, Phase::Error, message)
    }

    fn end_session(&self, session_id: u64, phase: Phase, message: impl Into<String>) -> bool {
        let mut runtime = lock(&self.runtime);
        let is_current = runtime
            .active_session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        if !is_current {
            return false;
        }
        let session = runtime.active_session.take().unwrap();
        if let Some(pid) = session.target_pid {
            runtime.last_target_pid = Some(pid);
        }
        runtime.status = RuntimeStatus {
            phase,
            message: message.into(),
        };
        self.dictation_active.store(false, Ordering::Release);
        true
    }

    pub fn last_transcript(&self) -> Option<String> {
        lock(&self.runtime).last_transcript.clone()
    }

    pub fn last_original_transcript(&self) -> Option<String> {
        lock(&self.runtime).last_original_transcript.clone()
    }

    pub fn remember_target_pid(&self, pid: i32) {
        lock(&self.runtime).last_target_pid = Some(pid);
    }

    pub fn last_target_pid(&self) -> Option<i32> {
        lock(&self.runtime).last_target_pid
    }

    pub fn dictation_active_flag(&self) -> Arc<AtomicBool> {
        self.dictation_active.clone()
    }

    pub fn hotkey(&self) -> String {
        lock(&self.settings).hotkey.clone()
    }

    pub fn microphone_id(&self) -> Option<String> {
        lock(&self.settings).microphone_id.clone()
    }

    pub fn launch_at_login(&self) -> bool {
        lock(&self.settings).launch_at_login
    }

    pub fn accuracy_settings(&self) -> (Vec<DictionaryEntry>, bool) {
        let settings = lock(&self.settings);
        (settings.dictionary.clone(), settings.smart_cleanup)
    }

    pub fn dictionary_entries(&self) -> Vec<DictionaryEntry> {
        let mut entries = lock(&self.settings).dictionary.clone();
        entries.sort_by_cached_key(|entry| entry.written_form.to_lowercase());
        entries
    }

    pub fn replace_hotkey(&self, hotkey: String) -> Result<(), String> {
        self.update_settings(|settings| settings.hotkey = hotkey)
    }

    pub fn replace_microphone(&self, microphone_id: Option<String>) -> Result<(), String> {
        self.update_settings(|settings| settings.microphone_id = microphone_id)
    }

    pub fn replace_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        self.update_settings(|settings| settings.launch_at_login = enabled)
    }

    pub fn replace_smart_cleanup(&self, enabled: bool) -> Result<(), String> {
        self.update_settings(|settings| settings.smart_cleanup = enabled)
    }

    pub fn save_dictionary_entry(
        &self,
        original_written_form: Option<&str>,
        written_form: &str,
        spoken_form: Option<&str>,
    ) -> Result<Vec<DictionaryEntry>, String> {
        let entry = prepare_dictionary_entry(written_form, spoken_form)?;
        self.try_update_settings(|settings| {
            let mut entries = settings.dictionary.clone();
            if let Some(original) = original_written_form {
                let index = entries
                    .iter()
                    .position(|candidate| candidate.written_form.eq_ignore_ascii_case(original))
                    .ok_or_else(|| "That dictionary entry no longer exists.".to_string())?;
                entries[index] = entry;
            } else {
                if entries.len() >= MAX_DICTIONARY_ENTRIES {
                    return Err(format!(
                        "The personal dictionary supports up to {MAX_DICTIONARY_ENTRIES} entries."
                    ));
                }
                entries.push(entry);
            }
            validate_dictionary(&entries)?;
            settings.dictionary = entries;
            Ok(())
        })?;
        Ok(self.dictionary_entries())
    }

    pub fn delete_dictionary_entry(
        &self,
        written_form: &str,
    ) -> Result<Vec<DictionaryEntry>, String> {
        self.try_update_settings(|settings| {
            let index = settings
                .dictionary
                .iter()
                .position(|entry| entry.written_form.eq_ignore_ascii_case(written_form))
                .ok_or_else(|| "That dictionary entry no longer exists.".to_string())?;
            settings.dictionary.remove(index);
            Ok(())
        })?;
        Ok(self.dictionary_entries())
    }

    fn update_settings(&self, update: impl FnOnce(&mut AppSettings)) -> Result<(), String> {
        self.try_update_settings(|settings| {
            update(settings);
            Ok(())
        })
    }

    fn try_update_settings(
        &self,
        update: impl FnOnce(&mut AppSettings) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut current = lock(&self.settings);
        let mut next = current.clone();
        update(&mut next)?;
        settings::save(&self.settings_path, &next)?;
        *current = next;
        Ok(())
    }

    pub fn hotkey_registered(&self) -> bool {
        self.hotkey_registered.load(Ordering::Acquire)
    }

    pub fn mark_hotkey_registered(&self, registered: bool) {
        self.hotkey_registered.store(registered, Ordering::Release);
    }

    pub fn begin_hotkey_capture(&self) {
        self.capturing_hotkey.store(true, Ordering::Release);
    }

    pub fn cancel_hotkey_capture(&self) {
        self.capturing_hotkey.store(false, Ordering::Release);
    }

    pub fn take_hotkey_capture(&self) -> bool {
        self.capturing_hotkey.swap(false, Ordering::AcqRel)
    }

    pub fn is_capturing_hotkey(&self) -> bool {
        self.capturing_hotkey.load(Ordering::Acquire)
    }

    pub fn snapshot(&self, accessibility_trusted: bool, microphone_trusted: bool) -> Snapshot {
        let runtime = lock(&self.runtime);
        let settings = lock(&self.settings);
        Snapshot {
            phase: runtime.status.phase.as_str(),
            message: runtime.status.message.clone(),
            recording_mode: runtime
                .active_session
                .as_ref()
                .map(|session| session.mode.as_str()),
            hotkey: settings.hotkey.clone(),
            capturing_hotkey: self.is_capturing_hotkey(),
            accessibility_trusted,
            microphone_trusted,
            microphone_id: settings.microphone_id.clone(),
            launch_at_login: settings.launch_at_login,
            smart_cleanup: settings.smart_cleanup,
            dictionary_count: settings.dictionary.len(),
            has_last_transcript: runtime.last_transcript.is_some(),
            has_original_transcript: runtime.last_original_transcript.is_some(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn state() -> AppState {
        AppState::new(
            PathBuf::from("unused-settings.json"),
            AppSettings::default(),
        )
    }

    #[test]
    fn cancellation_returns_to_ready_and_invalidates_session() {
        let state = state();
        let session = state
            .reserve_session(DictationMode::PushToTalk, Some(42))
            .unwrap();
        assert!(state.is_current_session(session.id));
        assert!(state.cancel_session());
        assert!(!state.is_current_session(session.id));
        assert_eq!(state.status().phase, Phase::Ready);
        assert!(session.cancel.load(Ordering::Acquire));
    }

    #[test]
    fn stale_completion_cannot_end_a_new_session() {
        let state = state();
        let first = state
            .reserve_session(DictationMode::PushToTalk, Some(1))
            .unwrap();
        assert!(state.cancel_session());
        let second = state
            .reserve_session(DictationMode::HandsFree, Some(2))
            .unwrap();

        assert!(!state.fail_session(first.id, "late failure"));
        assert!(state.is_current_session(second.id));
        assert_eq!(state.status().phase, Phase::Starting);
    }

    #[test]
    fn last_transcript_is_only_staged_by_current_session() {
        let state = state();
        let first = state
            .reserve_session(DictationMode::PushToTalk, None)
            .unwrap();
        assert!(state.cancel_session());
        let second = state
            .reserve_session(DictationMode::PushToTalk, None)
            .unwrap();

        assert!(!state.stage_transcript(first.id, "stale".to_string(), None));
        assert!(state.stage_transcript(second.id, "current".to_string(), None));
        assert_eq!(state.last_transcript().as_deref(), Some("current"));
    }

    #[test]
    fn unrelated_status_cannot_interrupt_an_active_session() {
        let state = state();
        let session = state
            .reserve_session(DictationMode::PushToTalk, None)
            .unwrap();

        state.set_status(Phase::Ready, "preference saved");

        assert!(state.is_current_session(session.id));
        assert_eq!(state.status().phase, Phase::Starting);
        assert_eq!(state.status().message, "Starting microphone…");
    }

    #[test]
    fn dictionary_updates_are_validated_and_saved_privately() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fnscribe-settings-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("settings.json");
        let state = AppState::new(path.clone(), AppSettings::default());

        let entries = state
            .save_dictionary_entry(None, "FnScribe", Some("fn scribe"))
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].written_form, "FnScribe");
        assert!(
            state
                .save_dictionary_entry(None, "Another", Some("FN SCRIBE"))
                .is_err()
        );
        assert_eq!(settings::load(&path).dictionary, entries);

        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        state.delete_dictionary_entry("fnscribe").unwrap();
        assert!(state.dictionary_entries().is_empty());
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn original_transcript_is_kept_only_when_cleanup_changed_it() {
        let state = state();
        let session = state
            .reserve_session(DictationMode::PushToTalk, None)
            .unwrap();
        assert!(state.stage_transcript(
            session.id,
            "Clean text ".to_string(),
            Some("Um clean text".to_string()),
        ));
        assert_eq!(state.last_transcript().as_deref(), Some("Clean text "));
        assert_eq!(
            state.last_original_transcript().as_deref(),
            Some("Um clean text")
        );
    }
}
