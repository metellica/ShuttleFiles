use std::path::{Path, PathBuf};

use crate::archive;
use crate::error::{AppError, AppResult};
use crate::fs::path;

/// Extensions that open as archives, so the frontend decides which rows
/// are enterable from the same list the backend dispatches on.
#[tauri::command]
pub fn archive_extensions() -> Vec<String> {
    archive::extensions()
}

/// Extract one member to a scratch folder and return the extracted
/// file, which is how a file inside an archive is opened for viewing.
#[tauri::command]
pub async fn archive_open_member(path: String) -> AppResult<String> {
    let (archive_path, inner) = path::split_archive(&path)
        .map(|(a, i)| (PathBuf::from(a), i.to_string()))
        .ok_or_else(|| AppError::InvalidPath(format!("Not inside an archive: {}", path)))?;
    if inner.is_empty() {
        return Err(AppError::InvalidPath("No member selected".into()));
    }

    tauri::async_runtime::spawn_blocking(move || {
        archive::extract_to_temp(&archive_path, &inner)
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| AppError::Io(format!("Cannot open the member: {}", e)))?
}

/// Suggested archive path for `sources` in `dir`: the single source's
/// name, or the folder's name when several were picked.
#[tauri::command]
pub fn archive_suggest_name(dir: String, sources: Vec<String>, extension: String) -> String {
    let stem = match sources.as_slice() {
        [only] => Path::new(only)
            .file_stem()
            .or_else(|| Path::new(only).file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "archive".into()),
        _ => {
            let name = path::display_name(&dir);
            if name.is_empty() || name == "This PC" {
                "archive".to_string()
            } else {
                name.trim_end_matches(['\\', '/', ':']).to_string()
            }
        }
    };
    format!("{}.{}", stem, extension)
}
