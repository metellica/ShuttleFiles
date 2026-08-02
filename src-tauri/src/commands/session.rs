use crate::config::session::{self, TabSnapshot, ViewSettings};
use crate::error::AppResult;

#[tauri::command]
pub fn load_tabs() -> AppResult<Vec<TabSnapshot>> {
    session::load_tabs()
}

#[tauri::command]
pub fn save_tabs(tabs: Vec<TabSnapshot>) -> AppResult<()> {
    session::save_tabs(&tabs)
}

#[tauri::command]
pub fn load_view_settings() -> AppResult<ViewSettings> {
    session::load_view_settings()
}

#[tauri::command]
pub fn save_view_settings(settings: ViewSettings) -> AppResult<()> {
    session::save_view_settings(&settings)
}
