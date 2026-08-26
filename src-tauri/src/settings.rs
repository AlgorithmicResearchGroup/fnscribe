use crate::accuracy::DictionaryEntry;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const DEFAULT_HOTKEY: &str = "Fn";
const SETTINGS_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppSettings {
    pub version: u32,
    pub hotkey: String,
    pub microphone_id: Option<String>,
    pub launch_at_login: bool,
    pub smart_cleanup: bool,
    pub dictionary: Vec<DictionaryEntry>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            hotkey: DEFAULT_HOTKEY.to_string(),
            microphone_id: None,
            launch_at_login: false,
            smart_cleanup: true,
            dictionary: Vec::new(),
        }
    }
}

pub fn load(path: &Path) -> AppSettings {
    secure_existing_storage(path);
    let mut settings: AppSettings = fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default();
    settings.version = SETTINGS_VERSION;
    settings
}

fn secure_existing_storage(path: &Path) {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent()
            && parent.is_dir()
        {
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
        if path.is_file() {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }
}

pub fn save(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the settings directory: {error}"))?;
        #[cfg(unix)]
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Could not secure the settings directory: {error}"))?;
    }
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("Could not serialize settings: {error}"))?;
    let temporary_path = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary_path)
        .map_err(|error| format!("Could not open the temporary settings file: {error}"))?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not secure the settings file: {error}"))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not write settings: {error}"))?;
    fs::rename(&temporary_path, path).map_err(|error| format!("Could not save settings: {error}"))
}

pub fn path_in(config_dir: PathBuf) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_legacy_hotkey_only_settings() {
        let settings: AppSettings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.hotkey, "Control+Space");
        assert_eq!(settings.microphone_id, None);
        assert!(!settings.launch_at_login);
        assert!(settings.smart_cleanup);
        assert!(settings.dictionary.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn loading_migrates_legacy_storage_to_owner_only_permissions() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fnscribe-permissions-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let path = directory.join("settings.json");
        fs::write(&path, r#"{"hotkey":"Fn"}"#).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let _ = load(&path);

        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
