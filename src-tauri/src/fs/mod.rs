pub mod drives;
pub mod local;
pub mod path;

use serde::{Deserialize, Serialize};

/// One row in the file list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    /// Unix seconds; 0 when unavailable.
    pub modified: u64,
    /// Lowercase extension without the dot, empty for directories.
    pub ext: String,
}

/// A drive / volume shown at "This PC" and on the fast dial.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    /// Root path, e.g. `C:\`.
    pub path: String,
    /// Volume label, e.g. `Windows`. Empty when unlabeled.
    pub label: String,
    /// `fixed` | `removable` | `network` | `cdrom` | `ramdisk` | `unknown`
    pub kind: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// A well-known folder (Desktop, Downloads, …) for the sidebar and fast dial.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceEntry {
    pub name: String,
    pub path: String,
    pub icon: String,
}

/// Directory listing plus the metadata the UI needs to render a header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub display_name: String,
    pub parent: Option<String>,
    pub is_virtual_root: bool,
    pub entries: Vec<FileEntry>,
}
