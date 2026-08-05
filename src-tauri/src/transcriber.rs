use std::path::{Path, PathBuf};
use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const MODEL_FILENAME: &str = "ggml-base.en-q5_1.bin";

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
        *self.context.lock().unwrap() = Some(context);
        *self.load_error.lock().unwrap() = None;
        Ok(())
    }

    pub fn set_load_error(&self, error: String) {
        *self.load_error.lock().unwrap() = Some(error);
    }

    pub fn is_ready(&self) -> bool {
        self.context.lock().unwrap().is_some()
    }

    pub fn load_error(&self) -> Option<String> {
        self.load_error.lock().unwrap().clone()
    }

    pub fn transcribe(&self, audio: &[f32]) -> Result<String, String> {
        let context = self.context.lock().unwrap();
        let context = context
            .as_ref()
            .ok_or_else(|| "The local transcription model is not ready.".to_string())?;
        let mut state = context
            .create_state()
            .map_err(|error| format!("Could not create a transcription session: {error}"))?;

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
