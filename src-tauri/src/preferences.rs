use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

// --- DATA STRUCTURES ---

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserPreferences {
    pub general: GeneralPreferences,
    pub interactions: Vec<InteractionEvent>,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            general: GeneralPreferences::default(),
            interactions: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeneralPreferences {
    pub default_transition_duration: f64,
    pub auto_ripple_edits: bool,
}

impl Default for GeneralPreferences {
    fn default() -> Self {
        Self {
            default_transition_duration: 0.5,
            auto_ripple_edits: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InteractionEvent {
    pub timestamp: i64,
    pub event_type: String,
    pub details: Value,
}

// --- MANAGER ---

pub struct PreferenceManager {
    preferences: Mutex<UserPreferences>,
    file_path: PathBuf,
}

impl PreferenceManager {
    pub fn new(app_handle: &AppHandle) -> Self {
        let app_dir = app_handle
            .path()
            .app_config_dir()
            .expect("failed to get app config dir");

        // Ensure config dir exists
        if !app_dir.exists() {
            let _ = fs::create_dir_all(&app_dir);
        }

        let file_path = app_dir.join("preferences.json");

        let preferences = if file_path.exists() {
            match fs::read_to_string(&file_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => UserPreferences::default(),
            }
        } else {
            UserPreferences::default()
        };

        Self {
            preferences: Mutex::new(preferences),
            file_path,
        }
    }

    pub fn new_in_memory() -> Self {
        Self {
            preferences: Mutex::new(UserPreferences::default()),
            file_path: std::path::PathBuf::from(""),
        }
    }

    pub fn save(&self) {
        let prefs = self.preferences.lock().unwrap();
        let json = serde_json::to_string_pretty(&*prefs).unwrap_or_default();
        // Ignore write errors for now, or log them
        let _ = fs::write(&self.file_path, json);
    }

    pub fn log_interaction(&self, event_type: &str, details: Value) {
        let mut prefs = self.preferences.lock().unwrap();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        prefs.interactions.push(InteractionEvent {
            timestamp,
            event_type: event_type.to_string(),
            details,
        });

        // Drop lock before saving to avoid holding it during I/O
        drop(prefs);
        self.save();
    }

    pub fn get_preferences(&self) -> UserPreferences {
        let prefs = self.preferences.lock().unwrap();
        prefs.clone()
    }
}
