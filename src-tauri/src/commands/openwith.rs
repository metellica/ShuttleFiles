use crate::config::openwith::{self, OpenWithSettings};
use crate::error::AppResult;
use crate::shell::{launch, vscode};

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

/// Opens a file or folder with `program`, or with the system default
/// when it is omitted. Unlike the opener plugin this starts the handler
/// in the item's own folder, which is what a batch file calling its
/// neighbours by relative path needs.
#[tauri::command]
pub async fn open_entry(path: String, program: Option<String>) -> AppResult<()> {
    launch::open_async(path, program).await
}

/// Whether the VS Code menu entry is worth offering at all.
#[tauri::command]
pub fn vscode_available() -> bool {
    vscode::locate().is_some()
}

/// Opens a whole selection in one VS Code window.
#[tauri::command]
pub async fn open_in_vscode(paths: Vec<String>) -> AppResult<()> {
    vscode::open_async(paths).await
}
