//! UI state that survives a restart: the open tabs and the view settings.
//!
//! These used to live in the WebView's localStorage, which buries them in
//! an opaque AppData folder. Keeping them next to `favorites.json` means
//! every setting the app owns sits in one inspectable directory.

use serde::{Deserialize, Serialize};

use crate::config::{read_json, write_json};
use crate::error::AppResult;

const TABS_FILE: &str = "tabs.json";
const VIEW_FILE: &str = "view.json";

/// How a tab is pinned to its folder. Mirrors the frontend `TabLock`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabLock {
    #[default]
    None,
    Locked,
    LockedAllowDirs,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabSnapshot {
    pub path: String,
    #[serde(default)]
    pub lock: TabLock,
    /// Base folder of a locked tab; empty when unlocked.
    #[serde(default)]
    pub locked_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSettings {
    pub row_scale: f64,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self { row_scale: 1.0 }
    }
}

pub fn load_tabs() -> AppResult<Vec<TabSnapshot>> {
    read_json(TABS_FILE)
}

pub fn save_tabs(tabs: &[TabSnapshot]) -> AppResult<()> {
    write_json(TABS_FILE, &tabs)
}

pub fn load_view_settings() -> AppResult<ViewSettings> {
    read_json(VIEW_FILE)
}

pub fn save_view_settings(settings: &ViewSettings) -> AppResult<()> {
    write_json(VIEW_FILE, settings)
}
