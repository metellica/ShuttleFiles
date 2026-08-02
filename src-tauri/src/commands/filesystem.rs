use crate::error::{AppError, AppResult};
use crate::fs::path;
use crate::fs::{local, DirListing};

#[tauri::command]
pub async fn list_dir(path: String) -> AppResult<DirListing> {
    // An archive browses like a folder, so the split happens here rather
    // than in every caller.
    if let Some((archive, inner)) = path::split_archive(&path) {
        let (archive, inner) = (archive.to_string(), inner.to_string());
        let listing = tauri::async_runtime::spawn_blocking(move || {
            crate::archive::list_dir(std::path::Path::new(&archive), &inner)
        })
        .await
        .map_err(|e| AppError::Io(format!("Cannot read the archive: {}", e)))??;

        return Ok(DirListing {
            display_name: path::display_name(&path),
            parent: path::parent_of(&path),
            is_virtual_root: false,
            entries: listing,
            path,
        });
    }
    local::list_dir(&path).await
}

/// Resolve whatever the user typed in the address bar into a real
/// directory. Returns an error (rather than navigating nowhere) so the
/// address bar can flash red and keep the text for editing.
#[tauri::command]
pub async fn resolve_path(input: String) -> AppResult<String> {
    let normalized = path::normalize_input(&input);
    if let Some((archive, _)) = path::split_archive(&normalized) {
        if std::path::Path::new(archive).is_file() {
            return Ok(normalized);
        }
    }
    if local::is_dir(&normalized).await {
        return Ok(normalized);
    }
    // Pointing at a file is a common paste; open its containing folder —
    // unless the file is an archive, which opens as one.
    let p = std::path::Path::new(&normalized);
    if p.is_file() {
        if crate::archive::is_archive(&normalized) {
            return Ok(path::archive_path(&normalized, ""));
        }
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
