//! JSON config store.
//!
//! Same convention as ShuttleSFTP: plain JSON under `~/.config/shuttle-files/`
//! on every platform, so settings are easy to inspect, diff and sync.

pub mod favorites;
pub mod openwith;
pub mod session;

use std::path::PathBuf;

use crate::error::{AppError, AppResult};

pub fn config_dir() -> AppResult<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AppError::Config("Cannot determine home directory".into()))?;
    let dir = home.join(".config").join("shuttle-files");
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Config(format!("Cannot create config dir: {}", e)))?;
    Ok(dir)
}

/// Read a config file, falling back to the default when it is missing.
/// A corrupt file is an error rather than silent data loss.
pub fn read_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> AppResult<T> {
    let file = config_dir()?.join(name);
    if !file.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(&file)
        .map_err(|e| AppError::Config(format!("Cannot read {}: {}", name, e)))?;
    serde_json::from_str(&raw)
        .map_err(|e| AppError::Config(format!("Cannot parse {}: {}", name, e)))
}

/// Write via a temp file + rename so a crash mid-write cannot truncate
/// the user's bookmarks.
pub fn write_json<T: serde::Serialize>(name: &str, value: &T) -> AppResult<()> {
    let dir = config_dir()?;
    let file = dir.join(name);
    let tmp = dir.join(format!("{}.tmp", name));
    let raw = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, raw)
        .map_err(|e| AppError::Config(format!("Cannot write {}: {}", name, e)))?;
    std::fs::rename(&tmp, &file)
        .map_err(|e| AppError::Config(format!("Cannot commit {}: {}", name, e)))
}
