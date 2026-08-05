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
    /// Which side of a vertical split the tab belongs to. Absent in
    /// files written before the split existed, which is exactly the
    /// single-pane layout `0` describes.
    #[serde(default)]
    pub pane: u32,
    /// The tab that was in front on its side.
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSettings {
    pub row_scale: f64,
    /// Share of the width the left pane takes when the view is split.
    #[serde(default = "default_split_ratio")]
    pub split_ratio: f64,
}

fn default_split_ratio() -> f64 {
    0.5
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            row_scale: 1.0,
            split_ratio: default_split_ratio(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `read_json` treats a file it cannot parse as an error rather than
    /// silent data loss, so every field added later has to be optional or
    /// the first start after an update loses the user's tabs.
    #[test]
    fn a_snapshot_from_before_the_split_still_parses() {
        let old = r#"[{"path":"C:\\Users","lock":"none","lockedPath":""}]"#;
        let tabs: Vec<TabSnapshot> = serde_json::from_str(old).expect("parse");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].pane, 0, "an old tab belongs to the only pane there was");
        assert!(!tabs[0].active);
    }

    #[test]
    fn view_settings_from_before_the_split_still_parse() {
        let old = r#"{"rowScale":1.4}"#;
        let view: ViewSettings = serde_json::from_str(old).expect("parse");
        assert_eq!(view.row_scale, 1.4);
        assert_eq!(view.split_ratio, 0.5, "an unsplit window starts even");
    }

    #[test]
    fn a_split_snapshot_round_trips() {
        let tabs = vec![
            TabSnapshot {
                path: "C:\\Users".into(),
                active: true,
                ..Default::default()
            },
            TabSnapshot {
                path: "D:\\work".into(),
                pane: 1,
                active: true,
                ..Default::default()
            },
        ];
        let raw = serde_json::to_string(&tabs).unwrap();
        let back: Vec<TabSnapshot> = serde_json::from_str(&raw).unwrap();
        assert_eq!(back[1].pane, 1);
        assert!(back[0].active && back[1].active, "each side keeps its own front tab");
    }
}
