//! Local (and UNC) file system access.
//!
//! Listings run on a blocking thread: `std::fs` on a big directory is
//! CPU/IO bound and would otherwise stall the async runtime.

use std::time::UNIX_EPOCH;

use crate::error::{AppError, AppResult};
use crate::fs::path;
use crate::fs::{DirListing, FileEntry};

#[cfg(windows)]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
#[cfg(windows)]
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

fn is_hidden(name: &str, meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let _ = name;
        let attrs = meta.file_attributes();
        attrs & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        name.starts_with('.')
    }
}

fn extension_of(name: &str, is_dir: bool) -> String {
    if is_dir {
        return String::new();
    }
    match name.rfind('.') {
        // A leading dot is part of the name (".gitignore"), not an extension.
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn to_entry(dir: &str, name: String, meta: &std::fs::Metadata, is_symlink: bool) -> FileEntry {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let is_dir = meta.is_dir();
    FileEntry {
        path: path::join(dir, &name),
        ext: extension_of(&name, is_dir),
        is_hidden: is_hidden(&name, meta),
        is_symlink,
        is_dir,
        size: if is_dir { 0 } else { meta.len() },
        modified,
        name,
    }
}

/// Directories first, then case-insensitive name — Explorer's default.
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn read_dir_blocking(dir: &str) -> AppResult<Vec<FileEntry>> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| AppError::Io(format!("Cannot open {}: {}", dir, e)))?;

    let mut entries = Vec::new();
    for item in rd {
        // A single unreadable entry must not abort the whole listing.
        let Ok(item) = item else { continue };
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        // DirEntry::metadata does not traverse symlinks, so it can never
        // hang on a dead link. The target is only resolved when the entry
        // really is a link, and a broken target falls back to the link itself.
        let Ok(link_meta) = item.metadata() else {
            continue;
        };
        let is_symlink = link_meta.file_type().is_symlink();
        let meta = if is_symlink {
            std::fs::metadata(item.path()).unwrap_or(link_meta)
        } else {
            link_meta
        };
        entries.push(to_entry(dir, name, &meta, is_symlink));
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// List a directory, or the drive list when at the virtual root.
pub async fn list_dir(dir: &str) -> AppResult<DirListing> {
    let dir = dir.to_string();

    let entries = if path::is_virtual_root(&dir) {
        crate::fs::drives::list_drives()
            .await
            .into_iter()
            .map(|d| FileEntry {
                name: if d.label.is_empty() {
                    path::display_name(&d.path)
                } else {
                    format!("{} ({})", d.label, d.path.trim_end_matches(['\\', '/']))
                },
                path: d.path,
                is_dir: true,
                is_symlink: false,
                is_hidden: false,
                size: 0,
                modified: 0,
                ext: String::new(),
            })
            .collect()
    } else {
        let d = dir.clone();
        tokio::task::spawn_blocking(move || read_dir_blocking(&d))
            .await
            .map_err(|e| AppError::Io(format!("Listing task failed: {}", e)))??
    };

    Ok(DirListing {
        display_name: path::display_name(&dir),
        parent: path::parent_of(&dir),
        is_virtual_root: path::is_virtual_root(&dir),
        path: dir,
        entries,
    })
}

/// Whether a path exists and is a directory (used by the address bar).
pub async fn is_dir(p: &str) -> bool {
    if path::is_virtual_root(p) {
        return true;
    }
    let p = p.to_string();
    tokio::task::spawn_blocking(move || std::path::Path::new(&p).is_dir())
        .await
        .unwrap_or(false)
}

pub async fn create_dir(p: &str) -> AppResult<()> {
    let p = p.to_string();
    let target = p.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir(&target))
        .await
        .map_err(|e| AppError::Io(format!("Task failed: {}", e)))?
        .map_err(|e| AppError::Io(format!("Cannot create {}: {}", p, e)))
}

pub async fn rename(from: &str, to: &str) -> AppResult<()> {
    let (from, to) = (from.to_string(), to.to_string());
    tokio::task::spawn_blocking(move || std::fs::rename(&from, &to))
        .await
        .map_err(|e| AppError::Io(format!("Task failed: {}", e)))?
        .map_err(|e| AppError::Io(format!("Cannot rename: {}", e)))
}
