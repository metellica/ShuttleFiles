use crate::config::favorites::{self, Favorite, RecentEntry};
use crate::error::AppResult;
use crate::fs::{drives, DriveInfo, PlaceEntry};

#[tauri::command]
pub async fn list_drives() -> Vec<DriveInfo> {
    drives::list_drives().await
}

/// Well-known folders for the sidebar. Entries whose directory does not
/// exist on this machine are dropped rather than shown as dead links.
#[tauri::command]
pub fn quick_access() -> Vec<PlaceEntry> {
    let candidates: Vec<(&str, Option<std::path::PathBuf>, &str)> = vec![
        ("Home", dirs::home_dir(), "🏠"),
        ("Desktop", dirs::desktop_dir(), "🖥"),
        ("Documents", dirs::document_dir(), "📄"),
        ("Downloads", dirs::download_dir(), "⬇"),
        ("Pictures", dirs::picture_dir(), "🖼"),
        ("Music", dirs::audio_dir(), "🎵"),
        ("Videos", dirs::video_dir(), "🎬"),
    ];

    candidates
        .into_iter()
        .filter_map(|(name, dir, icon)| {
            let dir = dir?;
            if !dir.is_dir() {
                return None;
            }
            Some(PlaceEntry {
                name: name.to_string(),
                path: dir.to_string_lossy().to_string(),
                icon: icon.to_string(),
            })
        })
        .collect()
}

#[tauri::command]
pub fn list_favorites() -> AppResult<Vec<Favorite>> {
    favorites::list_favorites()
}

#[tauri::command]
pub fn add_favorite(
    path: String,
    name: Option<String>,
    icon: Option<String>,
) -> AppResult<Vec<Favorite>> {
    favorites::add_favorite(&path, name, icon)
}

#[tauri::command]
pub fn remove_favorite(id: String) -> AppResult<Vec<Favorite>> {
    favorites::remove_favorite(&id)
}

#[tauri::command]
pub fn reorder_favorites(ids: Vec<String>) -> AppResult<Vec<Favorite>> {
    favorites::reorder_favorites(ids)
}

#[tauri::command]
pub fn list_recent() -> AppResult<Vec<RecentEntry>> {
    favorites::list_recent()
}

#[tauri::command]
pub fn record_visit(path: String) -> AppResult<()> {
    favorites::record_visit(&path)
}

#[tauri::command]
pub fn clear_recent() -> AppResult<()> {
    favorites::clear_recent()
}
