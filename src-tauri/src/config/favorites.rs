//! Favorites (⭐ bookmarks) and visit history, which together feed the
//! fast dial shown on a new tab.

use serde::{Deserialize, Serialize};

use crate::config::{read_json, write_json};
use crate::error::AppResult;
use crate::fs::path;

const FAVORITES_FILE: &str = "favorites.json";
const RECENT_FILE: &str = "recent.json";
/// Keeping the whole history would grow unbounded and slow the fast
/// dial; the tail is never shown anyway.
const RECENT_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Favorite {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Emoji shown on the fast dial tile.
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub path: String,
    pub name: String,
    pub visits: u32,
    /// Unix seconds.
    pub last_visit: u64,
}

pub fn list_favorites() -> AppResult<Vec<Favorite>> {
    read_json(FAVORITES_FILE)
}

/// Add a favorite, or update the alias if the path is already saved.
pub fn add_favorite(path_str: &str, name: Option<String>, icon: Option<String>) -> AppResult<Vec<Favorite>> {
    let mut list: Vec<Favorite> = read_json(FAVORITES_FILE)?;
    let name = name.unwrap_or_else(|| path::display_name(path_str));
    match list.iter_mut().find(|f| f.path == path_str) {
        Some(existing) => {
            existing.name = name;
            if let Some(icon) = icon {
                existing.icon = icon;
            }
        }
        None => list.push(Favorite {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path: path_str.to_string(),
            icon: icon.unwrap_or_else(|| "📁".to_string()),
        }),
    }
    write_json(FAVORITES_FILE, &list)?;
    Ok(list)
}

pub fn remove_favorite(id: &str) -> AppResult<Vec<Favorite>> {
    let mut list: Vec<Favorite> = read_json(FAVORITES_FILE)?;
    list.retain(|f| f.id != id);
    write_json(FAVORITES_FILE, &list)?;
    Ok(list)
}

/// Persist a new order after a drag-and-drop reorder on the fast dial.
pub fn reorder_favorites(ids: Vec<String>) -> AppResult<Vec<Favorite>> {
    let list: Vec<Favorite> = read_json(FAVORITES_FILE)?;
    let mut sorted: Vec<Favorite> = Vec::with_capacity(list.len());
    for id in &ids {
        if let Some(f) = list.iter().find(|f| &f.id == id) {
            sorted.push(f.clone());
        }
    }
    // Anything the client didn't know about keeps its relative position.
    for f in &list {
        if !ids.contains(&f.id) {
            sorted.push(f.clone());
        }
    }
    write_json(FAVORITES_FILE, &sorted)?;
    Ok(sorted)
}

pub fn list_recent() -> AppResult<Vec<RecentEntry>> {
    read_json(RECENT_FILE)
}

/// Record a visit. Called on every navigation, so it must stay cheap.
pub fn record_visit(path_str: &str) -> AppResult<()> {
    if path::is_virtual_root(path_str) {
        return Ok(());
    }
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match list.iter_mut().find(|r| r.path == path_str) {
        Some(entry) => {
            entry.visits += 1;
            entry.last_visit = now;
        }
        None => list.push(RecentEntry {
            path: path_str.to_string(),
            name: path::display_name(path_str),
            visits: 1,
            last_visit: now,
        }),
    }

    list.sort_by(|a, b| b.last_visit.cmp(&a.last_visit));
    list.truncate(RECENT_LIMIT);
    write_json(RECENT_FILE, &list)
}

pub fn clear_recent() -> AppResult<()> {
    write_json(RECENT_FILE, &Vec::<RecentEntry>::new())
}
