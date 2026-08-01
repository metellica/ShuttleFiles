use crate::error::{AppError, AppResult};
use crate::fs::path;
use crate::fs::{local, DirListing};

#[tauri::command]
pub async fn list_dir(path: String) -> AppResult<DirListing> {
    local::list_dir(&path).await
}

/// Resolve whatever the user typed in the address bar into a real
/// directory. Returns an error (rather than navigating nowhere) so the
/// address bar can flash red and keep the text for editing.
#[tauri::command]
pub async fn resolve_path(input: String) -> AppResult<String> {
    let normalized = path::normalize_input(&input);
    if local::is_dir(&normalized).await {
        return Ok(normalized);
    }
    // Pointing at a file is a common paste; open its containing folder.
    let p = std::path::Path::new(&normalized);
    if p.is_file() {
        if let Some(parent) = path::parent_of(&normalized) {
            return Ok(parent);
        }
    }
    Err(AppError::InvalidPath(format!(
        "Not a folder: {}",
        normalized
    )))
}

#[tauri::command]
pub fn parent_path(path: String) -> Option<String> {
    path::parent_of(&path)
}

#[tauri::command]
pub fn breadcrumbs(path: String) -> Vec<(String, String)> {
    path::breadcrumbs(&path)
}

#[tauri::command]
pub fn home_dir() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| path::ROOT.to_string())
}

#[tauri::command]
pub async fn create_dir(path: String) -> AppResult<()> {
    local::create_dir(&path).await
}

#[tauri::command]
pub async fn rename_entry(from: String, to: String) -> AppResult<()> {
    local::rename(&from, &to).await
}
