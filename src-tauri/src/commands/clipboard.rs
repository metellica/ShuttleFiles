use crate::error::AppResult;
use crate::shell::clipboard::{self, ClipboardFiles};

#[cfg(windows)]
fn clipboard_owner(window: &tauri::Window) -> AppResult<isize> {
    window
        .hwnd()
        .map(|hwnd| hwnd.0 as isize)
        .map_err(|e| crate::error::AppError::Io(format!("Cannot get window handle: {}", e)))
}

#[cfg(not(windows))]
fn clipboard_owner(_window: &tauri::Window) -> AppResult<isize> {
    Ok(0)
}

/// Put the selection on the system clipboard. `cut` marks it as a move,
/// which Explorer honours through the `Preferred DropEffect` format.
#[tauri::command]
pub async fn clipboard_write_files(
    paths: Vec<String>,
    cut: bool,
    window: tauri::Window,
) -> AppResult<()> {
    clipboard::write_files(paths, cut, clipboard_owner(&window)?).await
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

#[tauri::command]
pub async fn clipboard_write_text(text: String, window: tauri::Window) -> AppResult<()> {
    clipboard::write_text(text, clipboard_owner(&window)?).await
}

#[tauri::command]
pub async fn clipboard_read_text() -> AppResult<String> {
    clipboard::read_text().await
}
