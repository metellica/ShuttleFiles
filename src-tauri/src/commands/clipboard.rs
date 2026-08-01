use crate::error::AppResult;
use crate::shell::clipboard::{self, ClipboardFiles};

/// Put the selection on the system clipboard. `cut` marks it as a move,
/// which Explorer honours through the `Preferred DropEffect` format.
#[tauri::command]
pub async fn clipboard_write_files(paths: Vec<String>, cut: bool) -> AppResult<()> {
    clipboard::write_files(paths, cut).await
}

#[tauri::command]
pub async fn clipboard_read_files() -> AppResult<ClipboardFiles> {
    clipboard::read_files().await
}

/// Cheap check used to enable/disable the Paste menu item.
#[tauri::command]
pub async fn clipboard_has_files() -> bool {
    clipboard::has_files().await
}
