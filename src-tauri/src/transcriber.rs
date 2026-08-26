use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const MODEL_FILENAME: &str = "ggml-small.en-q5_1.bin";
const MAX_VOCABULARY_PROMPT_TOKENS: usize = 200;
const VOCABULARY_PROMPT_TOKENIZATION_CAP: usize = 1_024;

pub struct Transcriber {
    context: Mutex<Option<WhisperContext>>,
    load_error: Mutex<Option<String>>,
}

impl Transcriber {
    pub fn new() -> Self {
        Self {
            context: Mutex::new(None),
            load_error: Mutex::new(None),
        }
    }

    pub fn load(&self, path: &Path) -> Result<(), String> {
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|error| format!("Could not load the local model: {error}"))?;
        *lock(&self.context) = Some(context);
        *lock(&self.load_error) = None;
        Ok(())
    }

    pub fn set_load_error(&self, error: String) {
        *lock(&self.load_error) = Some(error);
    }

    pub fn is_ready(&self) -> bool {
        lock(&self.context).is_some()
    }

    pub fn load_error(&self) -> Option<String> {
        lock(&self.load_error).clone()
    }

    pub fn transcribe(
        &self,
        audio: &[f32],
        cancel: &AtomicBool,
        vocabulary_prompt: Option<&str>,
    ) -> Result<String, String> {
        let context = lock(&self.context);
        let context = context
            .as_ref()
            .ok_or_else(|| "The local transcription model is not ready.".to_string())?;
        let mut state = context
            .create_state()
            .map_err(|error| format!("Could not create a transcription session: {error}"))?;

        let prompt_tokens = vocabulary_prompt.and_then(|prompt| {
            context
                .tokenize(prompt, VOCABULARY_PROMPT_TOKENIZATION_CAP)
                .ok()
                .map(|mut tokens| {
                    if tokens.len() > MAX_VOCABULARY_PROMPT_TOKENS {
                        tokens.drain(..tokens.len() - MAX_VOCABULARY_PROMPT_TOKENS);
                    }
                    tokens
                })
        });
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism()
            .map(|count| count.get().min(8) as i32)
            .unwrap_or(4);
        params.set_n_threads(threads);
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        if let Some(tokens) = prompt_tokens.as_deref() {
            params.set_tokens(tokens);
        }
        // `full` is synchronous, and `cancel` outlives it. whisper.cpp only
        // reads this AtomicBool through the callback, so the raw pointer stays
        // valid and is never aliased for mutation.
        unsafe {
            params.set_abort_callback(Some(abort_if_cancelled));
            params.set_abort_callback_user_data(
                std::ptr::from_ref(cancel).cast_mut().cast::<c_void>(),
            );
        }

        state
            .full(params, audio)
            .map_err(|error| format!("Local transcription failed: {error}"))?;

        let text = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<String>();
        Ok(clean_transcript(&text))
    }
}

unsafe extern "C" fn abort_if_cancelled(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: `data` is installed immediately above from a live `AtomicBool`
    // and whisper.cpp invokes the callback only during the synchronous call.
    unsafe { &*data.cast::<AtomicBool>() }.load(Ordering::Acquire)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn find_model(resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("FNSCRIBE_MODEL") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(resource_dir) = resource_dir {
        candidates.push(resource_dir.join("resources/models").join(MODEL_FILENAME));
        candidates.push(resource_dir.join("models").join(MODEL_FILENAME));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models")
            .join(MODEL_FILENAME),
    );

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "Model missing. Run ./scripts/download-model.sh, then relaunch.".to_string())
}

fn clean_transcript(text: &str) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = cleaned
        .trim_matches(|character: char| {
            character.is_ascii_punctuation() || character.is_whitespace()
        })
        .to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "blank_audio" | "silence" | "no speech" | "music"
    ) {
        String::new()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_segment_spacing() {
        assert_eq!(clean_transcript(" Hello   there. "), "Hello there.");
    }

    #[test]
    fn removes_silence_markers() {
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcript("(silence)"), "");
    }
}
