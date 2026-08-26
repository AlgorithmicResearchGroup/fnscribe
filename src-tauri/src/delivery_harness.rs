use crate::platform::macos;
use serde::Serialize;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

const TEXT_ENV: &str = "FNSCRIBE_DELIVERY_HARNESS_TEXT";
const RESULT_ENV: &str = "FNSCRIBE_DELIVERY_HARNESS_RESULT";
const DELAY_ENV: &str = "FNSCRIBE_DELIVERY_HARNESS_DELAY_MS";
const DEFAULT_DELAY_MS: u64 = 3_000;
const MIN_DELAY_MS: u64 = 500;
const MAX_DELAY_MS: u64 = 30_000;
const MAX_TEST_TEXT_BYTES: usize = 4_096;

#[derive(Serialize)]
struct HarnessReport {
    success: bool,
    target_pid: Option<i32>,
    text_bytes: usize,
    error: Option<String>,
}

pub fn start(app: AppHandle) {
    let Ok(text) = std::env::var(TEXT_ENV) else {
        return;
    };
    let result_path = std::env::var_os(RESULT_ENV).map(PathBuf::from);
    let delay_ms = std::env::var(DELAY_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_DELAY_MS)
        .clamp(MIN_DELAY_MS, MAX_DELAY_MS);

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay_ms));
        let target_pid =
            macos::frontmost_application_pid().filter(|pid| *pid != std::process::id() as i32);
        let result = if text.is_empty() {
            Err("Harness text cannot be empty.".to_string())
        } else if text.len() > MAX_TEST_TEXT_BYTES {
            Err(format!(
                "Harness text must be {MAX_TEST_TEXT_BYTES} bytes or fewer."
            ))
        } else {
            target_pid
                .ok_or_else(|| "No external frontmost application was found.".to_string())
                .and_then(|pid| macos::insert_text(&app, pid, &text))
        };
        let report = HarnessReport {
            success: result.is_ok(),
            target_pid,
            text_bytes: text.len(),
            error: result.err(),
        };
        let success = report.success;
        let report = serde_json::to_string_pretty(&report)
            .expect("delivery harness reports contain only serializable values");
        if let Some(path) = result_path {
            let _ = std::fs::write(path, &report);
        }
        eprintln!("FnScribe delivery harness: {report}");
        app.exit(if success { 0 } else { 2 });
    });
}
