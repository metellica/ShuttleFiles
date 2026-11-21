//! Favorites (⭐ bookmarks) and visit history, which together feed the
//! fast dial shown on a new tab.

use serde::{Deserialize, Serialize};

use crate::config::{read_json, write_json};
use crate::error::AppResult;
use crate::fs::path;

const FAVORITES_FILE: &str = "favorites.json";
const RECENT_FILE: &str = "recent.json";
/// Keeping the whole history would grow unbounded; the tail is never
/// shown anyway. The per-host cap stops one busy machine from pushing
/// every other remote's folders out.
const RECENT_LIMIT: usize = 500;
const RECENT_PER_HOST_LIMIT: usize = 80;
/// A folder's weight halves every week, so what was busy ten days ago
/// fades without anyone having to un-star it.
const HALF_LIFE_SECS: f64 = 7.0 * 86_400.0;
/// Unpinned folders untouched this long are forgotten.
const STALE_SECS: u64 = 60 * 86_400;

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
    /// Kept out of the decay and eviction: shown first on its machine.
    #[serde(default)]
    pub pinned: bool,
    /// `\\server` for network paths, empty for local ones. Derived from
    /// `path` on every read, never trusted from disk.
    #[serde(default)]
    pub host: String,
    /// Frecency: visits weighted by how recently they happened. Derived.
    #[serde(default)]
    pub score: f64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn frecency(entry: &RecentEntry, now: u64) -> f64 {
    let age = now.saturating_sub(entry.last_visit) as f64;
    entry.visits as f64 * 0.5_f64.powf(age / HALF_LIFE_SECS)
}

/// Fill the derived fields and order the list best-first.
fn rank(list: &mut [RecentEntry], now: u64) {
    for e in list.iter_mut() {
        e.host = path::host_of(&e.path);
        e.score = frecency(e, now);
    }
    list.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
            .then(b.last_visit.cmp(&a.last_visit))
    });
}

/// Drop what has faded and enforce the per-host and total caps. Expects
/// a list already ordered by `rank`.
fn prune(list: Vec<RecentEntry>, now: u64) -> Vec<RecentEntry> {
    let mut per_host: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut kept = Vec::with_capacity(list.len().min(RECENT_LIMIT));
    for e in list {
        if !e.pinned && now.saturating_sub(e.last_visit) > STALE_SECS {
            continue;
        }
        let n = per_host.entry(e.host.clone()).or_insert(0);
        if !e.pinned && *n >= RECENT_PER_HOST_LIMIT {
            continue;
        }
        *n += 1;
        kept.push(e);
        if kept.len() >= RECENT_LIMIT {
            break;
        }
    }
    kept
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
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    rank(&mut list, now_secs());
    Ok(list)
}

fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Record a visit. Called on every navigation, so it must stay cheap.
pub fn record_visit(path_str: &str) -> AppResult<()> {
    if path::is_virtual_root(path_str) {
        return Ok(());
    }
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    let now = now_secs();

    match list.iter_mut().find(|r| same_path(&r.path, path_str)) {
        Some(entry) => {
            // Fold the decay into the count so a revived folder starts
            // from what it is worth today, not from its old peak.
            entry.visits = (frecency(entry, now).round() as u32).max(1) + 1;
            entry.last_visit = now;
        }
        None => list.push(RecentEntry {
            path: path_str.to_string(),
            name: path::display_name(path_str),
            visits: 1,
            last_visit: now,
            pinned: false,
            host: String::new(),
            score: 0.0,
        }),
    }

    rank(&mut list, now);
    write_json(RECENT_FILE, &prune(list, now))
}

/// Pin or unpin a folder; pinning one that has no history yet adds it.
pub fn toggle_pin(path_str: &str) -> AppResult<Vec<RecentEntry>> {
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    let now = now_secs();
    match list.iter_mut().find(|r| same_path(&r.path, path_str)) {
        Some(entry) => entry.pinned = !entry.pinned,
        None => list.push(RecentEntry {
            path: path_str.to_string(),
            name: path::display_name(path_str),
            visits: 1,
            last_visit: now,
            pinned: true,
            host: String::new(),
            score: 0.0,
        }),
    }
    rank(&mut list, now);
    write_json(RECENT_FILE, &list)?;
    Ok(list)
}

pub fn remove_recent(path_str: &str) -> AppResult<Vec<RecentEntry>> {
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    list.retain(|r| !same_path(&r.path, path_str));
    rank(&mut list, now_secs());
    write_json(RECENT_FILE, &list)?;
    Ok(list)
}

/// Forget the history but keep what the user pinned on purpose.
pub fn clear_recent() -> AppResult<Vec<RecentEntry>> {
    let mut list: Vec<RecentEntry> = read_json(RECENT_FILE)?;
    list.retain(|r| r.pinned);
    rank(&mut list, now_secs());
    write_json(RECENT_FILE, &list)?;
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, visits: u32, age_days: u64, pinned: bool, now: u64) -> RecentEntry {
        RecentEntry {
            path: path.into(),
            name: path::display_name(path),
            visits,
            last_visit: now - age_days * 86_400,
            pinned,
            host: String::new(),
            score: 0.0,
        }
    }

    #[test]
    fn recent_use_beats_old_heavy_use() {
        let now = 100 * 86_400;
        let mut list = vec![
            entry("C:\\old", 20, 30, false, now),
            entry("C:\\new", 3, 0, false, now),
        ];
        rank(&mut list, now);
        assert_eq!(list[0].path, "C:\\new");
    }

    #[test]
    fn pinned_first_and_survives_pruning() {
        let now = 100 * 86_400;
        let mut list = vec![
            entry("C:\\busy", 50, 0, false, now),
            entry("C:\\ancient", 1, 90, true, now),
            entry("C:\\stale", 9, 90, false, now),
        ];
        rank(&mut list, now);
        let kept = prune(list, now);
        let paths: Vec<_> = kept.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["C:\\ancient", "C:\\busy"]);
    }

    #[test]
    fn per_host_cap_leaves_room_for_other_hosts() {
        let now = 100 * 86_400;
        let mut list: Vec<_> = (0..RECENT_PER_HOST_LIMIT + 20)
            .map(|i| entry(&format!("\\\\nas\\s\\d{i}"), 5, 0, false, now))
            .collect();
        list.push(entry("\\\\other\\s\\x", 1, 1, false, now));
        rank(&mut list, now);
        let kept = prune(list, now);
        assert_eq!(kept.iter().filter(|e| e.host == "\\\\nas").count(), RECENT_PER_HOST_LIMIT);
        assert!(kept.iter().any(|e| e.host == "\\\\other"));
    }
}
