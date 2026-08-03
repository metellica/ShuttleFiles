//! Reading, browsing, extracting and creating archives.
//!
//! Formats are dispatched by extension to a purpose-built backend
//! rather than shelled out to 7-Zip: `zip` (deflate through `zlib-rs`,
//! plus bzip2/zstd/xz/lzma and AES), `sevenz-rust2` (multi-threaded
//! LZMA2) and `tar` over `flate2`/`bzip2`/`liblzma`/`zstd`. Everything
//! streams, so a 40 GB archive costs the same memory as a 40 KB one,
//! and every loop checks the job's cancel flag.

mod sevenz_format;
mod tar_format;
mod zip_format;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::fs::path::{self, SEP};
use crate::fs::FileEntry;
use crate::ops::engine::Progress;

/// Archive containers and the compressed single-file streams that are
/// close enough to browse the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Zip,
    SevenZ,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gz,
    Bz2,
    Xz,
    Zst,
}

impl Format {
    /// Extension used when this format names a new archive.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::SevenZ => "7z",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarBz2 => "tar.bz2",
            Format::TarXz => "tar.xz",
            Format::TarZst => "tar.zst",
            Format::Gz => "gz",
            Format::Bz2 => "bz2",
            Format::Xz => "xz",
            Format::Zst => "zst",
        }
    }

    /// A bare compressed stream holds exactly one file and cannot carry
    /// a directory tree.
    pub fn is_single_stream(self) -> bool {
        matches!(self, Format::Gz | Format::Bz2 | Format::Xz | Format::Zst)
    }
}

/// Every extension that opens as an archive, longest suffix first so
/// `.tar.gz` is recognised before `.gz`.
const EXTENSIONS: &[(&str, Format)] = &[
    ("tar.gz", Format::TarGz),
    ("tar.bz2", Format::TarBz2),
    ("tar.xz", Format::TarXz),
    ("tar.zst", Format::TarZst),
    ("tar.zstd", Format::TarZst),
    ("tgz", Format::TarGz),
    ("tbz", Format::TarBz2),
    ("tbz2", Format::TarBz2),
    ("txz", Format::TarXz),
    ("tzst", Format::TarZst),
    ("zip", Format::Zip),
    ("zipx", Format::Zip),
    ("jar", Format::Zip),
    ("war", Format::Zip),
    ("apk", Format::Zip),
    ("whl", Format::Zip),
    ("xpi", Format::Zip),
    ("epub", Format::Zip),
    ("7z", Format::SevenZ),
    ("tar", Format::Tar),
    ("gz", Format::Gz),
    ("bz2", Format::Bz2),
    ("xz", Format::Xz),
    ("zst", Format::Zst),
    ("zstd", Format::Zst),
];

/// The format `path` opens as, by extension. Content sniffing is
/// deliberately not used: a listing must not pay an IO round trip per
/// row just to decide whether a file is an archive.
pub fn detect(path: &str) -> Option<Format> {
    let lower = path.to_lowercase();
    EXTENSIONS
        .iter()
        .find(|(ext, _)| lower.ends_with(&format!(".{}", ext)))
        .map(|(_, format)| *format)
}

pub fn is_archive(path: &str) -> bool {
    detect(path).is_some()
}

/// A free path for a new archive, keeping compound extensions such as
/// `.tar.gz` in one piece: `pkg.tar.gz` becomes `pkg (2).tar.gz`, not
/// `pkg.tar (2).gz`.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower = name.to_lowercase();
    let (stem, ext) = EXTENSIONS
        .iter()
        .find(|(ext, _)| lower.ends_with(&format!(".{}", ext)))
        .map(|(ext, _)| (&name[..name.len() - ext.len() - 1], ext.to_string()))
        .unwrap_or((name.as_str(), String::new()));

    for n in 2..10_000 {
        let candidate = if ext.is_empty() {
            dir.join(format!("{} ({})", stem, n))
        } else {
            dir.join(format!("{} ({}).{}", stem, n, ext))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{}-{}", name, uuid::Uuid::new_v4()))
}

/// Extensions the frontend treats as archives, so the two sides cannot
/// drift apart.
pub fn extensions() -> Vec<String> {
    EXTENSIONS.iter().map(|(e, _)| (*e).to_string()).collect()
}

fn format_of(archive: &Path) -> AppResult<Format> {
    detect(&archive.to_string_lossy())
        .ok_or_else(|| AppError::InvalidPath(format!("Not an archive: {}", archive.display())))
}

/// One member of an archive. `path` is relative to the archive root and
/// uses the platform separator, so it slots straight into the virtual
/// paths the browser navigates with.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub packed: u64,
    /// Unix seconds; 0 when the format did not record one.
    pub modified: u64,
}

pub(crate) fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds from a civil date and time (UTC). Zip stores MS-DOS
/// date parts rather than an epoch, and pulling in a calendar crate for
/// one conversion is not worth it.
pub(crate) fn unix_from_civil(year: i64, month: i64, day: i64, hour: u64, min: u64, sec: u64) -> u64 {
    // Howard Hinnant's days_from_civil.
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    if days < 0 {
        return 0;
    }
    days as u64 * 86_400 + hour * 3_600 + min * 60 + sec
}

/// Normalise a name as stored in an archive: platform separators, no
/// leading or trailing slash, no `.` components. A `..` component is
/// the classic zip-slip, and rejects the member outright.
pub(crate) fn normalise_inner(name: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(&SEP.to_string()))
}

// --- Listing ----------------------------------------------------------------

/// Listings are cached because browsing a tree re-lists the same
/// archive for every level, and a central directory of a million
/// entries is not free to parse.
struct CachedListing {
    archive: PathBuf,
    modified: u64,
    size: u64,
    entries: Arc<Vec<Entry>>,
}

const CACHE_CAPACITY: usize = 4;

static CACHE: LazyLock<Mutex<Vec<CachedListing>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn stamp(archive: &Path) -> (u64, u64) {
    match std::fs::metadata(archive) {
        Ok(meta) => (meta.modified().map(unix_seconds).unwrap_or(0), meta.len()),
        Err(_) => (0, 0),
    }
}

/// Every member of `archive`, directories included.
pub fn list(archive: &Path) -> AppResult<Arc<Vec<Entry>>> {
    let (modified, size) = stamp(archive);
    if let Some(hit) = CACHE
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.archive == archive && c.modified == modified && c.size == size)
    {
        return Ok(hit.entries.clone());
    }

    let entries = Arc::new(match format_of(archive)? {
        Format::Zip => zip_format::list(archive)?,
        Format::SevenZ => sevenz_format::list(archive)?,
        format => tar_format::list(archive, format)?,
    });

    let mut cache = CACHE.lock().unwrap();
    cache.retain(|c| c.archive != archive);
    cache.push(CachedListing {
        archive: archive.to_path_buf(),
        modified,
        size,
        entries: entries.clone(),
    });
    if cache.len() > CACHE_CAPACITY {
        cache.remove(0);
    }
    Ok(entries)
}

/// One level of the archive, as file-list rows. Directories that only
/// exist implicitly (tar and zip may store no entry for them) are
/// synthesised, so navigation never hits a dead end.
pub fn list_dir(archive: &Path, inner: &str) -> AppResult<Vec<FileEntry>> {
    let entries = list(archive)?;
    let prefix = if inner.is_empty() {
        String::new()
    } else {
        format!("{}{}", inner, SEP)
    };
    let dir = path::archive_path(&archive.to_string_lossy(), inner);

    let mut level: BTreeMap<String, FileEntry> = BTreeMap::new();
    for entry in entries.iter() {
        let Some(rest) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let deeper = rest.contains(SEP);
        let name = match rest.split_once(SEP) {
            Some((head, _)) => head,
            None => rest,
        };
        // A member found below a folder proves the folder exists, but
        // says nothing about its size or date.
        if deeper && level.contains_key(name) {
            continue;
        }
        let is_dir = deeper || entry.is_dir;
        level.insert(
            name.to_string(),
            FileEntry {
                name: name.to_string(),
                path: path::join(&dir, name),
                is_dir,
                is_symlink: false,
                is_hidden: false,
                size: if deeper { 0 } else { entry.size },
                modified: if deeper { 0 } else { entry.modified },
                ext: if is_dir {
                    String::new()
                } else {
                    extension_of(name)
                },
            },
        );
    }
    Ok(level.into_values().collect())
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

// --- Selecting members ------------------------------------------------------

/// Which members a job touches, and where each one lands.
pub struct Selection {
    /// Inner paths picked by the user; empty means the whole archive.
    roots: Vec<String>,
}

impl Selection {
    pub fn new(roots: Vec<String>) -> Self {
        Self {
            roots: roots.into_iter().filter(|r| !r.is_empty()).collect(),
        }
    }

    pub fn all() -> Self {
        Self { roots: Vec::new() }
    }

    /// Destination path relative to the extraction root, or `None` when
    /// the member was not selected.
    pub(crate) fn output_for(&self, inner: &str) -> Option<PathBuf> {
        if self.roots.is_empty() {
            return safe_relative(inner);
        }
        for root in &self.roots {
            if inner == root {
                return safe_relative(leaf(root));
            }
            if let Some(rest) = inner.strip_prefix(&format!("{}{}", root, SEP)) {
                return safe_relative(&format!("{}{}{}", leaf(root), SEP, rest));
            }
        }
        None
    }
}

fn leaf(inner: &str) -> &str {
    inner.rsplit(SEP).next().unwrap_or(inner)
}

/// Reject anything that would write outside the destination directory.
fn safe_relative(inner: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(inner);
    if candidate.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(candidate)
}

/// Files and uncompressed bytes a job will process, for the progress bar.
pub fn measure(archive: &Path, selection: &Selection) -> AppResult<(u64, u64)> {
    let entries = list(archive)?;
    let mut files = 0;
    let mut bytes = 0;
    for entry in entries.iter() {
        if entry.is_dir || selection.output_for(&entry.path).is_none() {
            continue;
        }
        files += 1;
        bytes += entry.size;
    }
    Ok((files, bytes))
}

// --- Extracting -------------------------------------------------------------

pub fn extract(
    archive: &Path,
    selection: &Selection,
    dest: &Path,
    progress: &dyn Progress,
) -> AppResult<()> {
    let format = format_of(archive)?;
    std::fs::create_dir_all(dest)
        .map_err(|e| AppError::Io(format!("Cannot create {}: {}", dest.display(), e)))?;

    match format {
        Format::Zip => zip_format::extract(archive, selection, dest, progress),
        Format::SevenZ => sevenz_format::extract(archive, selection, dest, progress),
        format => tar_format::extract(archive, format, selection, dest, progress),
    }
}

/// Where members extracted only to be opened are put. One folder, so
/// what a run leaves behind can be found and cleared by a later one.
fn scratch_root() -> PathBuf {
    std::env::temp_dir().join("ShuttleFiles")
}

/// Age past which a scratch folder is assumed to belong to a finished
/// run rather than to a file someone still has open.
const SCRATCH_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Drop scratch folders left by earlier runs. Nothing tells us when the
/// program a member was handed to is done with it, so age is the only
/// safe signal — and a second instance running now must not delete what
/// the first one just extracted. Errors are ignored: a file still
/// locked is simply collected on a later start.
pub fn clean_scratch() {
    clean_scratch_in(&scratch_root(), SCRATCH_MAX_AGE);
}

fn clean_scratch_in(root: &Path, max_age: Duration) {
    // A junction in place of the root would make `read_dir` enumerate
    // somewhere else entirely, and the sweep below would delete what it
    // found there. Only a real directory is ever swept.
    match std::fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        _ => return,
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        // Only this module's own scratch folders, so a root shared with
        // something else — or an entry planted in it — is left alone.
        if uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let stale = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map(|age| age >= max_age)
            .unwrap_or(false);
        if !stale {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// Extract one member to a scratch folder and return the file's path,
/// which is how a file inside an archive is opened for viewing.
pub fn extract_to_temp(archive: &Path, inner: &str) -> AppResult<PathBuf> {
    let dest = scratch_root().join(uuid::Uuid::new_v4().to_string());
    extract(
        archive,
        &Selection::new(vec![inner.to_string()]),
        &dest,
        &NoProgress,
    )?;
    Ok(dest.join(leaf(inner)))
}

/// Progress sink for work the UI does not track, such as opening a
/// single member for viewing.
struct NoProgress;

impl Progress for NoProgress {
    fn add_bytes(&self, _n: u64) {}
    fn file_done(&self) {}
    fn set_current(&self, _name: &str) {}
    fn add_completed(&self, _files: u64, _bytes: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Create the destination's parent, then stream `reader` into it.
pub(crate) fn write_member(
    dest: &Path,
    relative: &Path,
    reader: &mut dyn Read,
    modified: u64,
    progress: &dyn Progress,
) -> AppResult<()> {
    let target = dest.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Io(format!("Cannot create {}: {}", parent.display(), e)))?;
    }
    if let Some(name) = relative.file_name().and_then(|n| n.to_str()) {
        progress.set_current(name);
    }

    let mut file = std::fs::File::create(&target)
        .map_err(|e| AppError::Io(format!("Cannot write {}: {}", target.display(), e)))?;
    let mut counted = ProgressReader::new(reader, progress);
    std::io::copy(&mut counted, &mut file).map_err(|e| io_error(&target, e))?;

    if modified > 0 {
        let _ = file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(modified));
    }
    progress.file_done();
    Ok(())
}

/// A cancelled job surfaces as an IO error deep inside a decoder; map it
/// back so the UI reports "Cancelled" rather than a spurious failure.
pub(crate) fn io_error(path: &Path, e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::Interrupted {
        return AppError::Cancelled;
    }
    AppError::Io(format!("{}: {}", path.display(), e))
}

/// Counts every byte that passes through and aborts on cancel, which is
/// what makes a single huge member interruptible.
pub(crate) struct ProgressReader<'a, R: ?Sized> {
    inner: &'a mut R,
    progress: &'a dyn Progress,
}

impl<'a, R: Read + ?Sized> ProgressReader<'a, R> {
    pub(crate) fn new(inner: &'a mut R, progress: &'a dyn Progress) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read + ?Sized> Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.progress.is_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            ));
        }
        let n = self.inner.read(buf)?;
        self.progress.add_bytes(n as u64);
        Ok(n)
    }
}

// --- Creating ---------------------------------------------------------------

/// A source file and the name it takes inside the archive.
pub struct Member {
    pub source: PathBuf,
    pub inner: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
}

/// Expand `sources` into the members an archive will hold. Each source
/// keeps its own name as the top-level entry, so compressing a folder
/// produces `folder/...` rather than a bag of loose files.
pub fn collect_members(sources: &[PathBuf], progress: &dyn Progress) -> AppResult<Vec<Member>> {
    let mut members = Vec::new();
    for source in sources {
        progress.check_cancel()?;
        let meta = std::fs::symlink_metadata(source)
            .map_err(|e| AppError::Io(format!("Cannot stat {}: {}", source.display(), e)))?;
        let base = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| AppError::InvalidPath(format!("Bad source: {}", source.display())))?;

        if !meta.is_dir() {
            members.push(Member {
                source: source.clone(),
                inner: base,
                is_dir: false,
                size: meta.len(),
                modified: meta.modified().map(unix_seconds).unwrap_or(0),
            });
            continue;
        }

        for entry in jwalk::WalkDir::new(source).skip_hidden(false) {
            progress.check_cancel()?;
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(source) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let meta = entry.metadata().ok();
            let is_dir = entry.file_type.is_dir();
            members.push(Member {
                inner: format!("{}{}{}", base, SEP, rel.to_string_lossy()),
                source: path,
                is_dir,
                size: if is_dir {
                    0
                } else {
                    meta.as_ref().map(|m| m.len()).unwrap_or(0)
                },
                modified: meta
                    .and_then(|m| m.modified().ok())
                    .map(unix_seconds)
                    .unwrap_or(0),
            });
        }
    }
    Ok(members)
}

pub fn create(
    archive: &Path,
    format: Format,
    members: &[Member],
    level: Option<i32>,
    progress: &dyn Progress,
) -> AppResult<()> {
    if format.is_single_stream() && members.iter().filter(|m| !m.is_dir).count() != 1 {
        return Err(AppError::InvalidPath(format!(
            ".{} holds a single file; select exactly one",
            format.extension()
        )));
    }
    if let Some(parent) = archive.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Io(format!("Cannot create {}: {}", parent.display(), e)))?;
    }

    let outcome = match format {
        Format::Zip => zip_format::create(archive, members, level, progress),
        Format::SevenZ => sevenz_format::create(archive, members, level, progress),
        format => tar_format::create(archive, format, members, level, progress),
    };
    if outcome.is_err() {
        // A half-written archive is worse than none: it looks openable.
        let _ = std::fs::remove_file(archive);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scratch directory removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join("shuttle-archive-tests")
                .join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Counter {
        files: std::sync::atomic::AtomicU64,
        bytes: std::sync::atomic::AtomicU64,
    }

    impl Default for Counter {
        fn default() -> Self {
            Self {
                files: std::sync::atomic::AtomicU64::new(0),
                bytes: std::sync::atomic::AtomicU64::new(0),
            }
        }
    }

    impl Progress for Counter {
        fn add_bytes(&self, n: u64) {
            self.bytes.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        }
        fn file_done(&self) {
            self.files.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        fn set_current(&self, _name: &str) {}
        fn add_completed(&self, _files: u64, _bytes: u64) {}
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    /// `tree/a.txt` and `tree/sub/b.txt`, the shape every round trip uses.
    fn make_tree(root: &Path) -> PathBuf {
        let tree = root.join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("a.txt"), b"aaaa").unwrap();
        std::fs::write(tree.join("sub").join("b.txt"), b"bbbbbb").unwrap();
        tree
    }

    fn round_trip(format: Format) {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let counter = Counter::default();

        let archive = tmp.path().join(format!("pkg.{}", format.extension()));
        let members = collect_members(&[tree.clone()], &counter).unwrap();
        create(&archive, format, &members, Some(6), &counter).unwrap();
        assert!(archive.is_file(), "{:?} was written", format);

        let listed = list(&archive).unwrap();
        let names: Vec<&str> = listed.iter().map(|e| e.path.as_str()).collect();
        assert!(
            names.contains(&format!("tree{}a.txt", SEP).as_str()),
            "{:?} lists members: {:?}",
            format,
            names
        );

        // The whole archive, then one folder out of it.
        let dest = tmp.path().join("out");
        extract(&archive, &Selection::all(), &dest, &counter).unwrap();
        assert_eq!(std::fs::read(dest.join("tree").join("a.txt")).unwrap(), b"aaaa");
        assert_eq!(
            std::fs::read(dest.join("tree").join("sub").join("b.txt")).unwrap(),
            b"bbbbbb"
        );

        let picked = tmp.path().join("picked");
        let selection = Selection::new(vec![format!("tree{}sub", SEP)]);
        extract(&archive, &selection, &picked, &counter).unwrap();
        assert_eq!(std::fs::read(picked.join("sub").join("b.txt")).unwrap(), b"bbbbbb");
        assert!(
            !picked.join("tree").exists(),
            "{:?} extracts the selection only",
            format
        );
    }

    #[test]
    fn zip_round_trip() {
        round_trip(Format::Zip);
    }

    #[test]
    fn sevenz_round_trip() {
        round_trip(Format::SevenZ);
    }

    #[test]
    fn tar_gz_round_trip() {
        round_trip(Format::TarGz);
    }

    #[test]
    fn tar_zst_round_trip() {
        round_trip(Format::TarZst);
    }

    #[test]
    fn tar_xz_round_trip() {
        round_trip(Format::TarXz);
    }

    #[test]
    fn tar_bz2_round_trip() {
        round_trip(Format::TarBz2);
    }

    #[test]
    fn plain_tar_round_trip() {
        round_trip(Format::Tar);
    }

    /// Browsing shows one level at a time, with folders that only exist
    /// implicitly filled in.
    #[test]
    fn listing_a_level_synthesises_missing_folders() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let counter = Counter::default();
        let archive = tmp.path().join("pkg.zip");
        let members = collect_members(&[tree], &counter).unwrap();
        create(&archive, Format::Zip, &members, Some(0), &counter).unwrap();

        let root = list_dir(&archive, "").unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, "tree");
        assert!(root[0].is_dir);

        let level = list_dir(&archive, "tree").unwrap();
        let names: Vec<&str> = level.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "sub"]);
        assert_eq!(level[0].size, 4);
        assert!(level[1].is_dir);
        // Rows carry the virtual path, so a double click can navigate.
        assert!(level[1].path.ends_with(&format!("pkg.zip!{}tree{}sub", SEP, SEP)));
    }

    #[test]
    fn measure_counts_only_the_selection() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let counter = Counter::default();
        let archive = tmp.path().join("pkg.zip");
        let members = collect_members(&[tree], &counter).unwrap();
        create(&archive, Format::Zip, &members, Some(0), &counter).unwrap();

        assert_eq!(measure(&archive, &Selection::all()).unwrap(), (2, 10));
        assert_eq!(
            measure(&archive, &Selection::new(vec![format!("tree{}sub", SEP)])).unwrap(),
            (1, 6)
        );
    }

    #[test]
    fn a_single_stream_holds_one_file() {
        let tmp = TempDir::new();
        let file = tmp.path().join("notes.txt");
        std::fs::write(&file, b"hello").unwrap();
        let counter = Counter::default();

        let archive = tmp.path().join("notes.txt.gz");
        let members = collect_members(&[file], &counter).unwrap();
        create(&archive, Format::Gz, &members, Some(6), &counter).unwrap();

        let listed = list(&archive).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "notes.txt");

        let dest = tmp.path().join("out");
        extract(&archive, &Selection::all(), &dest, &counter).unwrap();
        assert_eq!(std::fs::read(dest.join("notes.txt")).unwrap(), b"hello");
    }

    #[test]
    fn stale_scratch_folders_are_swept_and_fresh_ones_kept() {
        let tmp = TempDir::new();
        let root = tmp.path().join("scratch");
        let job = root.join(uuid::Uuid::new_v4().to_string());
        let member = job.join("notes.txt");
        std::fs::create_dir_all(&job).unwrap();
        std::fs::write(&member, b"hello").unwrap();

        clean_scratch_in(&root, Duration::from_secs(3600));
        assert!(member.exists(), "a folder this run may still be using");

        clean_scratch_in(&root, Duration::ZERO);
        assert!(!job.exists());
        assert!(root.exists(), "the root itself stays");
    }

    /// The root lives in the shared temp directory, so the sweep must
    /// touch nothing but the uuid folders this module creates there.
    #[test]
    fn sweeping_leaves_anything_that_is_not_our_scratch_folder() {
        let tmp = TempDir::new();
        let root = tmp.path().join("scratch");
        let other_dir = root.join("someone-elses");
        std::fs::create_dir_all(&other_dir).unwrap();
        let loose_file = root.join("stray.txt");
        std::fs::write(&loose_file, b"x").unwrap();

        clean_scratch_in(&root, Duration::ZERO);

        assert!(other_dir.exists());
        assert!(loose_file.exists());
    }

    /// A junction in place of the root would otherwise have `read_dir`
    /// enumerate — and the sweep delete from — its target.
    #[cfg(windows)]
    #[test]
    fn a_scratch_root_that_is_a_junction_is_not_swept() {
        let tmp = TempDir::new();
        let target = tmp.path().join("target");
        let planted = target.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&planted).unwrap();
        std::fs::write(planted.join("keep.txt"), b"keep").unwrap();

        let root = tmp.path().join("scratch");
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&root)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !made {
            return;
        }

        clean_scratch_in(&root, Duration::ZERO);

        assert!(planted.exists(), "the junction's target is untouched");
    }

    #[test]
    fn sweeping_a_scratch_root_that_was_never_created_is_not_an_error() {
        let tmp = TempDir::new();
        clean_scratch_in(&tmp.path().join("absent"), Duration::ZERO);
    }

    #[test]
    fn a_second_archive_of_the_same_name_does_not_clobber_the_first() {
        let tmp = TempDir::new();
        let first = tmp.path().join("pkg.tar.gz");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_path(&first), tmp.path().join("pkg (2).tar.gz"));
    }

    #[test]
    fn extensions_are_matched_longest_first() {
        assert_eq!(detect("a.tar.gz"), Some(Format::TarGz));
        assert_eq!(detect("a.TAR.GZ"), Some(Format::TarGz));
        assert_eq!(detect("a.gz"), Some(Format::Gz));
        assert_eq!(detect("a.7z"), Some(Format::SevenZ));
        assert_eq!(detect("notes.txt"), None);
    }

    #[test]
    fn traversal_out_of_the_destination_is_refused() {
        assert_eq!(normalise_inner("../../etc/passwd"), None);
        assert_eq!(normalise_inner("a/./b"), Some(format!("a{}b", SEP)));
        assert_eq!(normalise_inner("/"), None);
    }

    #[test]
    fn selecting_a_folder_keeps_its_name_at_the_root() {
        let selection = Selection::new(vec![format!("docs{}sub", SEP)]);
        assert_eq!(
            selection.output_for(&format!("docs{}sub{}a.txt", SEP, SEP)),
            Some(PathBuf::from(format!("sub{}a.txt", SEP)))
        );
        assert_eq!(selection.output_for(&format!("docs{}other.txt", SEP)), None);
    }

    #[test]
    fn extracting_everything_keeps_the_full_layout() {
        let selection = Selection::all();
        assert_eq!(
            selection.output_for(&format!("docs{}a.txt", SEP)),
            Some(PathBuf::from(format!("docs{}a.txt", SEP)))
        );
    }
}
