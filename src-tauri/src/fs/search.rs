//! Fuzzy file search over a directory, in the spirit of `fd` piped into
//! `fzf`.
//!
//! Two modes share everything but the walk: the toolbar filter looks at
//! one directory, "Find in Folder" descends the whole tree. Ranking,
//! case handling and highlighting come from [`crate::fs::fuzzy`], so a
//! query behaves identically either way.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::fs::fuzzy::Query;
use crate::fs::{local, path, FileEntry};

/// Enough to fill any screen many times over; ranking makes the tail
/// irrelevant, and an unbounded list would only cost memory and IPC time.
const DEFAULT_LIMIT: usize = 500;
const MAX_LIMIT: usize = 5_000;
/// A stubborn tree (or a symlink loop the walker cannot see through)
/// must not spin forever if nobody cancels.
const MAX_SCANNED: u64 = 2_000_000;
/// Cancellation is checked in batches; an atomic load per entry would
/// show up in the profile of a large tree.
const CANCEL_CHECK_INTERVAL: u64 = 512;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    #[serde(flatten)]
    pub entry: FileEntry,
    /// Path relative to the search root; equal to the name when not
    /// recursive. This is what was matched and what gets highlighted.
    pub rel: String,
    pub score: i32,
    /// Char (not byte) indices into `rel`, for highlighting.
    pub positions: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    /// Total matches found, which may exceed `hits.len()`.
    pub total: u64,
    /// Entries examined, so the UI can say why a search was truncated.
    pub scanned: u64,
    pub truncated: bool,
    pub cancelled: bool,
}

/// A hit plus its sort key, kept out of `SearchHit` so the ordering rules
/// live in one place. Ordered naturally: greater is a better match.
///
/// The key must be a total order, not just "score first": the collector
/// evicts by it and the final sort uses it, and if the two disagreed,
/// which of several equally-scored hits survived a truncation would be
/// arbitrary.
struct Ranked {
    hit: SearchHit,
    /// Precomputed, because `chars().count()` on every comparison would
    /// make the heap quadratic in path length.
    len: usize,
}

impl Ranked {
    fn new(hit: SearchHit) -> Self {
        Self {
            len: hit.rel.chars().count(),
            hit,
        }
    }

    /// Higher score wins; ties go to the shorter path, as in fzf, because
    /// a match filling more of the name is the more specific one, and
    /// finally to name order so the result is reproducible.
    fn cmp_key(&self) -> (i32, std::cmp::Reverse<usize>, std::cmp::Reverse<&str>) {
        (
            self.hit.score,
            std::cmp::Reverse(self.len),
            std::cmp::Reverse(self.hit.rel.as_str()),
        )
    }
}

impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_key() == other.cmp_key()
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp_key().cmp(&other.cmp_key())
    }
}

/// Bounded top-N collector.
///
/// The heap holds `Reverse`d hits so its root is the *weakest* one kept
/// so far, which is the only candidate worth evicting.
struct TopHits {
    heap: std::collections::BinaryHeap<std::cmp::Reverse<Ranked>>,
    limit: usize,
    total: u64,
}

impl TopHits {
    fn new(limit: usize) -> Self {
        Self {
            heap: std::collections::BinaryHeap::with_capacity(limit.min(1024) + 1),
            limit,
            total: 0,
        }
    }

    fn push(&mut self, hit: SearchHit) {
        self.total += 1;
        let candidate = Ranked::new(hit);
        if self.heap.len() < self.limit {
            self.heap.push(std::cmp::Reverse(candidate));
            return;
        }
        match self.heap.peek() {
            Some(std::cmp::Reverse(weakest)) if candidate > *weakest => {
                self.heap.pop();
                self.heap.push(std::cmp::Reverse(candidate));
            }
            _ => {}
        }
    }

    fn into_sorted(self) -> Vec<SearchHit> {
        let mut ranked: Vec<Ranked> = self.heap.into_iter().map(|r| r.0).collect();
        // Best first, by the same key the collector evicted on.
        ranked.sort_by(|a, b| b.cmp(a));
        ranked.into_iter().map(|r| r.hit).collect()
    }
}

/// Path of `full` relative to `root`, with the separator style preserved.
fn relative_to(root: &str, full: &str) -> String {
    let trimmed = root.trim_end_matches(['\\', '/']);
    if full.len() > trimmed.len() && full.as_bytes()[..trimmed.len()].eq_ignore_ascii_case(trimmed.as_bytes())
    {
        full[trimmed.len()..].trim_start_matches(['\\', '/']).to_string()
    } else {
        path::display_name(full)
    }
}

fn scan_one_level(
    root: &str,
    query: &Query,
    hits: &mut TopHits,
    scanned: &mut u64,
    cancel: &AtomicBool,
) -> AppResult<()> {
    let rd = std::fs::read_dir(root)
        .map_err(|e| AppError::Io(format!("Cannot search {}: {}", root, e)))?;
    for item in rd {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Ok(item) = item else { continue };
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        *scanned += 1;
        let Some((score, positions)) = query.score(&name) else {
            continue;
        };
        let Ok(link_meta) = item.metadata() else {
            continue;
        };
        let is_symlink = link_meta.file_type().is_symlink();
        let meta = if is_symlink {
            std::fs::metadata(item.path()).unwrap_or(link_meta)
        } else {
            link_meta
        };
        let full = path::join(root, &name);
        hits.push(SearchHit {
            entry: local::entry_at(full, name.clone(), &meta, is_symlink),
            rel: name,
            score,
            positions,
        });
    }
    Ok(())
}

/// Returns whether the walk stopped early because it hit the scan cap.
fn scan_tree(
    root: &str,
    query: &Query,
    hits: &mut TopHits,
    scanned: &mut u64,
    cancel: &AtomicBool,
) -> bool {
    // Links are not followed: a junction pointing at an ancestor would
    // otherwise turn the walk into an infinite one.
    let walker = jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .into_iter();

    for item in walker {
        if *scanned % CANCEL_CHECK_INTERVAL == 0 && cancel.load(Ordering::Relaxed) {
            return false;
        }
        if *scanned >= MAX_SCANNED {
            return true;
        }
        // An unreadable subtree is skipped, not fatal; a search that dies
        // on the first protected folder would be useless on a system drive.
        let Ok(item) = item else { continue };
        if item.depth() == 0 {
            continue;
        }
        *scanned += 1;

        let full = item.path().to_string_lossy().to_string();
        let rel = relative_to(root, &full);
        // The cheap subsequence test rejects almost everything, so the
        // dynamic program only runs on plausible candidates.
        if !query.is_candidate(&rel) {
            continue;
        }
        let Some((score, positions)) = query.score(&rel) else {
            continue;
        };
        let Ok(name) = item.file_name().to_os_string().into_string() else {
            continue;
        };
        let Ok(meta) = item.metadata() else { continue };
        let is_symlink = item.path_is_symlink();
        hits.push(SearchHit {
            entry: local::entry_at(full, name, &meta, is_symlink),
            rel,
            score,
            positions,
        });
    }
    false
}

/// Run a search to completion on the calling (blocking) thread.
pub fn search_blocking(
    root: &str,
    query_text: &str,
    recursive: bool,
    limit: usize,
    cancel: Arc<AtomicBool>,
) -> AppResult<SearchResult> {
    let query = Query::new(query_text);
    let limit = limit.clamp(1, MAX_LIMIT);
    let mut hits = TopHits::new(limit);
    let mut scanned = 0u64;

    if query.is_empty() {
        return Ok(SearchResult {
            hits: Vec::new(),
            total: 0,
            scanned: 0,
            truncated: false,
            cancelled: false,
        });
    }

    let truncated = if recursive {
        scan_tree(root, &query, &mut hits, &mut scanned, &cancel)
    } else {
        scan_one_level(root, &query, &mut hits, &mut scanned, &cancel)?;
        false
    };

    let total = hits.total;
    Ok(SearchResult {
        hits: hits.into_sorted(),
        total,
        scanned,
        truncated: truncated || total as usize > limit,
        cancelled: cancel.load(Ordering::Relaxed),
    })
}

/// Search off the async runtime; a deep tree would block it for seconds.
pub async fn search(
    root: String,
    query: String,
    recursive: bool,
    limit: usize,
    cancel: Arc<AtomicBool>,
) -> AppResult<SearchResult> {
    tokio::task::spawn_blocking(move || search_blocking(&root, &query, recursive, limit, cancel))
        .await
        .map_err(|e| AppError::Io(format!("Search task failed: {}", e)))?
}

pub const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;

    /// Same hand-rolled helper the IPC tests use, to avoid a dev-dependency
    /// for four directories.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("shuttle-files-search-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sep(p: &str) -> String {
        p.replace('\\', std::path::MAIN_SEPARATOR_STR)
    }

    fn fixture() -> TempDir {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::write(root.join("readme.md"), b"x").unwrap();
        std::fs::write(root.join("cargo.toml"), b"x").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), b"x").unwrap();
        std::fs::write(root.join("src").join("search.rs"), b"x").unwrap();
        std::fs::create_dir(root.join("src").join("deep")).unwrap();
        std::fs::write(root.join("src").join("deep").join("buried.rs"), b"x").unwrap();
        dir
    }

    fn run(root: &str, q: &str, recursive: bool) -> SearchResult {
        search_blocking(root, q, recursive, 500, Arc::new(AtomicBool::new(false))).unwrap()
    }

    #[test]
    fn a_shallow_search_ignores_subdirectories() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        // "buried" only exists two levels down.
        assert_eq!(run(&root, "buried", false).total, 0);
        assert!(run(&root, "buried", true).total > 0);
    }

    #[test]
    fn a_shallow_search_still_finds_top_level_entries() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let names: Vec<String> = run(&root, "cargo", false)
            .hits
            .into_iter()
            .map(|h| h.entry.name)
            .collect();
        assert_eq!(names, vec!["cargo.toml"]);
    }

    #[test]
    fn a_recursive_search_finds_nested_files() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let names: Vec<String> = run(&root, "buried", true)
            .hits
            .into_iter()
            .map(|h| h.entry.name)
            .collect();
        assert_eq!(names, vec!["buried.rs"]);
    }

    #[test]
    fn results_are_relative_to_the_search_root() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let hit = run(&root, "buried", true).hits.remove(0);
        assert_eq!(hit.rel, sep("src\\deep\\buried.rs"));
        assert!(hit.entry.path.ends_with("buried.rs"));
    }

    #[test]
    fn hits_come_back_best_first() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let hits = run(&root, "main", true).hits;
        assert_eq!(hits.first().unwrap().entry.name, "main.rs");
        assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn positions_line_up_with_the_relative_path() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let hit = run(&root, "buried", true).hits.remove(0);
        let chars: Vec<char> = hit.rel.chars().collect();
        let matched: String = hit.positions.iter().map(|&i| chars[i]).collect();
        assert_eq!(matched, "buried");
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_everything() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let result = run(&root, "   ", true);
        assert!(result.hits.is_empty());
        assert_eq!(result.total, 0);
    }

    #[test]
    fn a_cancelled_search_stops_early_and_says_so() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let cancel = Arc::new(AtomicBool::new(true));
        let result = search_blocking(&root, "rs", true, 500, cancel).unwrap();
        assert!(result.cancelled);
        assert!(result.hits.is_empty());
    }

    #[test]
    fn the_limit_caps_the_returned_hits_but_not_the_count() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let result =
            search_blocking(&root, "s", true, 1, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.total > 1, "total was {}", result.total);
        assert!(result.truncated);
    }

    #[test]
    fn the_best_hit_survives_a_tight_limit() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let capped =
            search_blocking(&root, "main", true, 1, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(capped.hits[0].entry.name, "main.rs");
    }

    #[test]
    fn a_tight_limit_keeps_the_best_hits_not_an_arbitrary_batch() {
        // Regression guard: the bounded collector once evicted the *best*
        // hit instead of the worst, so a truncated search returned the
        // dregs. Needs more matches than the limit to catch it.
        let dir = TempDir::new();
        let root = dir.path();
        for i in 0..40 {
            // Every name matches "report" fuzzily, but only one is exact.
            std::fs::write(root.join(format!("r{}-e-p-o-r-t-{}.log", i, i)), b"x").unwrap();
        }
        std::fs::write(root.join("report.txt"), b"x").unwrap();
        let root = root.to_string_lossy().to_string();

        let full = run(&root, "report", false);
        assert!(full.total > 5, "fixture should overflow the limit");
        let best: Vec<String> = full.hits.iter().take(5).map(|h| h.rel.clone()).collect();
        assert_eq!(best[0], "report.txt");

        let capped =
            search_blocking(&root, "report", false, 5, Arc::new(AtomicBool::new(false))).unwrap();
        let capped_names: Vec<String> = capped.hits.iter().map(|h| h.rel.clone()).collect();
        assert_eq!(capped_names, best, "a capped search must return the top hits");
        assert!(capped.truncated);
    }

    #[test]
    fn the_same_holds_for_a_recursive_search() {
        let dir = TempDir::new();
        let root = dir.path();
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        for i in 0..30 {
            std::fs::write(nested.join(format!("c{}-o-n-f-i-g-{}.bak", i, i)), b"x").unwrap();
        }
        std::fs::write(nested.join("config.toml"), b"x").unwrap();
        let root = root.to_string_lossy().to_string();

        let capped =
            search_blocking(&root, "config", true, 3, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(capped.hits.len(), 3);
        assert!(
            capped.hits[0].rel.ends_with("config.toml"),
            "best hit was {}",
            capped.hits[0].rel
        );
    }

    #[test]
    fn a_wildcard_query_matches_by_extension() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join("readme.md"), b"x").unwrap();
        std::fs::create_dir(root.join("logs")).unwrap();
        std::fs::write(root.join("logs").join("run.txt"), b"x").unwrap();
        let root = root.to_string_lossy().to_string();

        let shallow: Vec<String> = run(&root, "*.txt", false)
            .hits
            .into_iter()
            .map(|h| h.rel)
            .collect();
        assert_eq!(shallow, vec!["notes.txt"]);

        let mut deep: Vec<String> = run(&root, "*.txt", true)
            .hits
            .into_iter()
            .map(|h| h.rel)
            .collect();
        deep.sort();
        assert_eq!(deep, vec![sep("logs\\run.txt"), "notes.txt".to_string()]);
    }

    #[test]
    fn a_wildcard_query_still_excludes_non_matches() {
        let dir = fixture();
        let root = dir.path().to_string_lossy().to_string();
        let names: Vec<String> = run(&root, "*.md", true)
            .hits
            .into_iter()
            .map(|h| h.entry.name)
            .collect();
        assert_eq!(names, vec!["readme.md"]);
    }

    #[test]
    fn a_missing_directory_is_an_error_not_a_panic() {
        let result = search_blocking(
            "Z:\\definitely\\not\\here",
            "x",
            false,
            10,
            Arc::new(AtomicBool::new(false)),
        );
        assert!(result.is_err());
    }

    #[test]
    fn relative_paths_are_computed_against_the_root() {
        assert_eq!(relative_to("C:\\a", "C:\\a\\b\\c.txt"), "b\\c.txt");
        assert_eq!(relative_to("C:\\a\\", "C:\\a\\b.txt"), "b.txt");
        // A path outside the root degrades to its name rather than lying.
        assert_eq!(relative_to("C:\\a", "D:\\other\\b.txt"), "b.txt");
    }
}
