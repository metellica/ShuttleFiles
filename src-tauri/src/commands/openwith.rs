use crate::config::openwith::{self, OpenWithSettings};
use crate::error::AppResult;

#[tauri::command]
pub fn load_open_with() -> AppResult<OpenWithSettings> {
    openwith::load()
}

/// Returns the normalised settings, which is what the UI should display.
#[tauri::command]
pub fn save_open_with(settings: OpenWithSettings) -> AppResult<OpenWithSettings> {
    openwith::save(settings)
}

#[tauri::command]
pub fn default_open_with() -> OpenWithSettings {
    OpenWithSettings::default()
}
