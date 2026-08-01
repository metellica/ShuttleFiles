//! Job execution: scan, then copy / move / delete with progress.
//!
//! Tuned for large trees (requirement R6):
//!
//! * the scan walks with `jwalk` and fetches sizes inside
//!   `process_read_dir`, so the per-file `stat` syscalls happen on the
//!   walker's thread pool instead of serially on one thread;
//! * files are copied/deleted by a bounded worker pool fed through a
//!   backpressured channel, which is a large win on SSD/NVMe and on
//!   network shares where each operation is latency-bound;
//! * on Windows each file goes through `CopyFile2`, so the copy runs in
//!   the kernel, keeps timestamps and attributes, reports byte-level
//!   progress through its callback and can be cancelled mid-file;
//! * same-volume moves are a single `rename` of the whole subtree.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jwalk::WalkDirGeneric;
use tauri::{AppHandle, Emitter, Runtime};

use crate::error::{AppError, AppResult};
use crate::ops::{Job, JobKind, JobStatus, OpsRegistry};

/// A busy copy would otherwise emit thousands of events per second and
/// drown the WebView in IPC traffic.
const EMIT_INTERVAL: Duration = Duration::from_millis(120);

/// Queue depth between the walker and the copy/delete workers. Deep
/// enough to keep workers fed, shallow enough to bound memory.
const WORK_QUEUE_DEPTH: usize = 1024;

/// Concurrent file operations. More helps on SSD/NVMe and on network
/// shares (latency-bound); a spinning disk would prefer 1, so the count
/// stays modest rather than tracking the core count upwards.
fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

/// Progress sink. Shared across worker threads, hence `&self` + `Sync`.
pub trait Progress: Sync {
    fn add_bytes(&self, n: u64);
    fn file_done(&self);
    fn set_current(&self, name: &str);
    /// Account for work that completed in one step (a same-volume
    /// rename moves a whole subtree at once).
    fn add_completed(&self, files: u64, bytes: u64);
    fn is_cancelled(&self) -> bool;

    /// `Err(AppError::Cancelled)` aborts the job at the next checkpoint.
    fn check_cancel(&self) -> AppResult<()> {
        if self.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        Ok(())
    }
}

struct EmitState {
    last_emit: Instant,
    last_bytes: u64,
    speed: f64,
}

struct Reporter<R: Runtime> {
    app: AppHandle<R>,
    job: Arc<Job>,
    // Counters are atomics so workers never contend on the state lock.
    done_files: AtomicU64,
    done_bytes: AtomicU64,
    current: Mutex<String>,
    emit: Mutex<EmitState>,
}

impl<R: Runtime> Reporter<R> {
    fn new(app: AppHandle<R>, job: Arc<Job>) -> Self {
        Self {
            app,
            job,
            done_files: AtomicU64::new(0),
            done_bytes: AtomicU64::new(0),
            current: Mutex::new(String::new()),
            emit: Mutex::new(EmitState {
                last_emit: Instant::now(),
                last_bytes: 0,
                speed: 0.0,
            }),
        }
    }

    /// Emit at most every [`EMIT_INTERVAL`], unless `force` (status
    /// change or completion), which must never be dropped.
    fn flush(&self, force: bool) {
        // If another worker is already emitting there is nothing to add.
        let Ok(mut emit) = self.emit.try_lock() else {
            return;
        };
        let now = Instant::now();
        let elapsed = now.duration_since(emit.last_emit);
        if !force && elapsed < EMIT_INTERVAL {
            return;
        }

        let done_bytes = self.done_bytes.load(Ordering::Relaxed);
        if elapsed.as_secs_f64() > 0.0 {
            let instant = done_bytes.saturating_sub(emit.last_bytes) as f64 / elapsed.as_secs_f64();
            // Smooth the reading so the number doesn't flicker.
            emit.speed = if emit.speed == 0.0 {
                instant
            } else {
                emit.speed * 0.7 + instant * 0.3
            };
        }
        emit.last_bytes = done_bytes;
        emit.last_emit = now;
        let speed = emit.speed as u64;
        drop(emit);

        let snapshot = {
            let mut state = self.job.state.lock().unwrap();
            state.done_files = self.done_files.load(Ordering::Relaxed);
            state.done_bytes = done_bytes;
            state.bytes_per_sec = speed;
            if let Ok(current) = self.current.try_lock() {
                state.current.clone_from(&current);
            }
            state.clone()
        };
        let _ = self.app.emit("fileop:update", snapshot);
    }

    fn mutate(&self, f: impl FnOnce(&mut crate::ops::JobState)) {
        f(&mut self.job.state.lock().unwrap());
    }

    /// Push the atomic counters into the shared state before a status
    /// change, so the final snapshot is consistent.
    fn sync_counters(&self) {
        let (files, bytes) = (
            self.done_files.load(Ordering::Relaxed),
            self.done_bytes.load(Ordering::Relaxed),
        );
        self.mutate(|s| {
            s.done_files = files;
            s.done_bytes = bytes;
        });
    }
}

impl<R: Runtime> Progress for Reporter<R> {
    fn add_bytes(&self, n: u64) {
        self.done_bytes.fetch_add(n, Ordering::Relaxed);
        self.flush(false);
    }

    fn file_done(&self) {
        self.done_files.fetch_add(1, Ordering::Relaxed);
        self.flush(false);
    }

    fn set_current(&self, name: &str) {
        // Cosmetic: skipping it under contention costs nothing.
        if let Ok(mut current) = self.current.try_lock() {
            current.clear();
            current.push_str(name);
        }
    }

    fn add_completed(&self, files: u64, bytes: u64) {
        self.done_files.fetch_add(files, Ordering::Relaxed);
        self.done_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.flush(true);
    }

    fn is_cancelled(&self) -> bool {
        self.job.is_cancelled()
    }
}

// --- Scanning ---------------------------------------------------------------

/// What one source contributes to a job.
#[derive(Debug, Default)]
pub struct SourcePlan {
    pub root: PathBuf,
    pub is_dir: bool,
    pub files: u64,
    pub bytes: u64,
    /// Directories below `root`, relative and shallow-first. Only
    /// directories are materialised — a tree with millions of files
    /// still has a manageable number of folders.
    pub dirs: Vec<PathBuf>,
}

/// `jwalk` walk whose per-entry state carries the file size, fetched on
/// the walker's own thread pool.
type SizedWalk = WalkDirGeneric<((), u64)>;

fn sized_walk(root: &Path) -> SizedWalk {
    WalkDirGeneric::<((), u64)>::new(root)
        .skip_hidden(false)
        .process_read_dir(|_depth, _path, _read_dir_state, children| {
            // Runs on jwalk's rayon pool: doing the `stat` here keeps
            // one syscall per file off the consuming thread, which is
            // what made the old sequential scan slow on big trees.
            for entry in children.iter_mut().flatten() {
                if !entry.file_type.is_dir() {
                    entry.client_state = entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        })
}

fn scan_source(root: &Path, progress: &dyn Progress) -> AppResult<SourcePlan> {
    let meta = std::fs::symlink_metadata(root)
        .map_err(|e| AppError::Io(format!("Cannot stat {}: {}", root.display(), e)))?;

    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Ok(SourcePlan {
            root: root.to_path_buf(),
            is_dir: false,
            files: 1,
            bytes: meta.len(),
            dirs: Vec::new(),
        });
    }

    let mut plan = SourcePlan {
        root: root.to_path_buf(),
        is_dir: true,
        ..Default::default()
    };

    for entry in sized_walk(root) {
        progress.check_cancel()?;
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path == root {
            continue;
        }
        if entry.file_type.is_dir() {
            if let Ok(rel) = path.strip_prefix(root) {
                plan.dirs.push(rel.to_path_buf());
            }
        } else {
            plan.files += 1;
            plan.bytes += entry.client_state;
        }
    }

    // Shallow-first, so creating them in order never needs a parent that
    // does not exist yet.
    plan.dirs.sort();
    Ok(plan)
}

fn scan_sources(sources: &[String], progress: &dyn Progress) -> AppResult<Vec<SourcePlan>> {
    sources
        .iter()
        .map(|s| scan_source(Path::new(s), progress))
        .collect()
}

// --- Parallel execution -----------------------------------------------------

/// Walk `root` and hand every file to `action` on a bounded worker pool.
///
/// The walker thread only enumerates; workers do the IO. The channel is
/// bounded, so a fast walker cannot outrun slow workers and blow up
/// memory on a huge tree.
fn for_each_file_parallel<F>(root: &Path, progress: &dyn Progress, action: F) -> AppResult<()>
where
    F: Fn(&Path, u64) -> AppResult<()> + Sync,
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<(PathBuf, u64)>(WORK_QUEUE_DEPTH);
    let rx = Mutex::new(rx);
    let first_error: Mutex<Option<AppError>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..worker_count() {
            scope.spawn(|| loop {
                let Ok((path, size)) = rx.lock().unwrap().recv() else {
                    break;
                };
                // Keep draining after a failure rather than exiting: the
                // walker may be parked on a full channel, and dropping
                // out here would deadlock it.
                if progress.is_cancelled() || first_error.lock().unwrap().is_some() {
                    continue;
                }
                if let Err(e) = action(&path, size) {
                    let mut slot = first_error.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(e);
                    }
                }
            });
        }

        for entry in sized_walk(root) {
            if progress.is_cancelled() || first_error.lock().unwrap().is_some() {
                break;
            }
            let Ok(entry) = entry else { continue };
            if entry.file_type.is_dir() {
                continue;
            }
            if tx.send((entry.path(), entry.client_state)).is_err() {
                break;
            }
        }
        drop(tx);
    });

    if let Some(e) = first_error.lock().unwrap().take() {
        return Err(e);
    }
    progress.check_cancel()
}

// --- Single-file copy -------------------------------------------------------

#[cfg(windows)]
mod native_copy {
    use std::ffi::c_void;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        CopyFile2, COPYFILE2_CALLBACK_CHUNK_FINISHED, COPYFILE2_EXTENDED_PARAMETERS,
        COPYFILE2_MESSAGE, COPYFILE2_MESSAGE_ACTION, COPYFILE2_PROGRESS_CANCEL,
        COPYFILE2_PROGRESS_CONTINUE, COPYFILE_FLAGS,
    };

    use super::Progress;
    use crate::error::{AppError, AppResult};

    struct Ctx<'a> {
        progress: &'a dyn Progress,
        /// `uliTotalBytesTransferred` is cumulative; we report deltas.
        last_transferred: u64,
    }

    unsafe extern "system" fn on_progress(
        message: *const COPYFILE2_MESSAGE,
        context: *const c_void,
    ) -> COPYFILE2_MESSAGE_ACTION {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ctx = unsafe { &mut *(context as *mut Ctx) };
            let message = unsafe { &*message };

            if message.Type == COPYFILE2_CALLBACK_CHUNK_FINISHED {
                let total = unsafe { message.Info.ChunkFinished.uliTotalBytesTransferred };
                ctx.progress
                    .add_bytes(total.saturating_sub(ctx.last_transferred));
                ctx.last_transferred = total;
            }

            if ctx.progress.is_cancelled() {
                COPYFILE2_PROGRESS_CANCEL
            } else {
                COPYFILE2_PROGRESS_CONTINUE
            }
        }));
        // Unwinding into the kernel's copy loop would abort the process.
        outcome.unwrap_or(COPYFILE2_PROGRESS_CANCEL)
    }

    fn wide(path: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn copy_file(
        src: &Path,
        dst: &Path,
        _size: u64,
        progress: &dyn Progress,
    ) -> AppResult<()> {
        let (src_w, dst_w) = (wide(src), wide(dst));
        let mut ctx = Ctx {
            progress,
            last_transferred: 0,
        };
        // No COPY_FILE_NO_BUFFERING: it is documented as "recommended for
        // very large file transfers", but measured ~24 MiB/s against
        // ~3.3 GiB/s for the default path on a 512 MiB file. CopyFile2
        // already switches to unbuffered IO on its own when that helps.
        let params = COPYFILE2_EXTENDED_PARAMETERS {
            dwSize: std::mem::size_of::<COPYFILE2_EXTENDED_PARAMETERS>() as u32,
            dwCopyFlags: COPYFILE_FLAGS(0),
            pfCancel: std::ptr::null_mut(),
            pProgressRoutine: Some(on_progress),
            pvCallbackContext: &mut ctx as *mut Ctx as *mut c_void,
        };

        unsafe {
            CopyFile2(
                PCWSTR(src_w.as_ptr()),
                PCWSTR(dst_w.as_ptr()),
                Some(&params),
            )
        }
        .map_err(|e| {
            // A cancelled copy surfaces as a generic failure; report the
            // cause the caller actually needs to act on.
            if progress.is_cancelled() {
                AppError::Cancelled
            } else {
                AppError::Io(format!("Cannot copy {}: {}", src.display(), e))
            }
        })
    }
}

#[cfg(not(windows))]
mod native_copy {
    use std::io::{Read, Write};
    use std::path::Path;

    use super::Progress;
    use crate::error::{AppError, AppResult};

    const CHUNK_SIZE: usize = 1 << 20;
    /// Below this, one `std::fs::copy` is faster than a manual loop and
    /// the progress granularity is fine anyway.
    const CHUNKED_THRESHOLD: u64 = 8 << 20;

    pub fn copy_file(
        src: &Path,
        dst: &Path,
        size: u64,
        progress: &dyn Progress,
    ) -> AppResult<()> {
        if size <= CHUNKED_THRESHOLD {
            std::fs::copy(src, dst)
                .map_err(|e| AppError::Io(format!("Cannot copy {}: {}", src.display(), e)))?;
            progress.add_bytes(size);
            return Ok(());
        }

        let meta = std::fs::metadata(src)
            .map_err(|e| AppError::Io(format!("Cannot stat {}: {}", src.display(), e)))?;
        let mut reader = std::fs::File::open(src)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", src.display(), e)))?;
        let mut writer = std::fs::File::create(dst)
            .map_err(|e| AppError::Io(format!("Cannot write {}: {}", dst.display(), e)))?;
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            progress.check_cancel()?;
            let n = reader
                .read(&mut buf)
                .map_err(|e| AppError::Io(format!("Cannot read {}: {}", src.display(), e)))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| AppError::Io(format!("Cannot write {}: {}", dst.display(), e)))?;
            progress.add_bytes(n as u64);
        }
        if let Ok(mtime) = meta.modified() {
            let _ = writer.set_modified(mtime);
        }
        Ok(())
    }
}

/// Copy one file, creating the destination's parent if the tree changed
/// since the scan.
fn copy_one(src: &Path, dst: &Path, size: u64, progress: &dyn Progress) -> AppResult<()> {
    match native_copy::copy_file(src, dst, size, progress) {
        Ok(()) => Ok(()),
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(first) => {
            let Some(parent) = dst.parent() else {
                return Err(first);
            };
            if parent.is_dir() || std::fs::create_dir_all(parent).is_err() {
                return Err(first);
            }
            native_copy::copy_file(src, dst, size, progress)
        }
    }
}

// --- Deleting ---------------------------------------------------------------

/// Remove a file, clearing the read-only attribute if that is what
/// blocked it — Explorer does the same rather than refusing.
fn remove_file_forcing(path: &Path) -> AppResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() != std::io::ErrorKind::PermissionDenied => {
            return Err(AppError::Io(format!(
                "Cannot delete {}: {}",
                path.display(),
                e
            )))
        }
        Err(_) => {}
    }

    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
    std::fs::remove_file(path)
        .map_err(|e| AppError::Io(format!("Cannot delete {}: {}", path.display(), e)))
}

fn delete_source(plan: &SourcePlan, progress: &dyn Progress) -> AppResult<()> {
    if !plan.is_dir {
        if let Some(name) = plan.root.file_name().and_then(|s| s.to_str()) {
            progress.set_current(name);
        }
        remove_file_forcing(&plan.root)?;
        progress.add_completed(plan.files, plan.bytes);
        return Ok(());
    }

    for_each_file_parallel(&plan.root, progress, |path, size| {
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            progress.set_current(name);
        }
        remove_file_forcing(path)?;
        progress.add_bytes(size);
        progress.file_done();
        Ok(())
    })?;

    // Directories are empty now; remove them deepest-first. Cheap
    // compared to the file pass, so it stays single-threaded.
    let mut dirs = plan.dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for rel in dirs {
        let _ = std::fs::remove_dir(plan.root.join(rel));
    }
    std::fs::remove_dir(&plan.root)
        .map_err(|e| AppError::Io(format!("Cannot delete {}: {}", plan.root.display(), e)))
}

// --- Copy / move ------------------------------------------------------------

/// Pick a non-colliding destination: `report.txt` -> `report (2).txt`.
fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 2..10_000 {
        let candidate_name = match ext {
            Some(ext) => format!("{} ({}).{}", stem, n, ext),
            None => format!("{} ({})", stem, n),
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{}-{}", name, uuid::Uuid::new_v4()))
}

/// Reject pasting a folder into itself or one of its descendants, which
/// would otherwise recurse until the disk fills up.
fn assert_not_nested(src: &Path, dst_dir: &Path) -> AppResult<()> {
    if dst_dir == src || dst_dir.starts_with(src) {
        return Err(AppError::InvalidPath(format!(
            "Cannot copy {} into itself",
            src.display()
        )));
    }
    Ok(())
}

fn copy_source(plan: &SourcePlan, dst: &Path, progress: &dyn Progress) -> AppResult<()> {
    if !plan.is_dir {
        if let Some(name) = plan.root.file_name().and_then(|s| s.to_str()) {
            progress.set_current(name);
        }
        copy_one(&plan.root, dst, plan.bytes, progress)?;
        progress.file_done();
        return Ok(());
    }

    std::fs::create_dir_all(dst)
        .map_err(|e| AppError::Io(format!("Cannot create {}: {}", dst.display(), e)))?;
    // Shallow-first, so every parent exists by the time it is needed.
    for rel in &plan.dirs {
        progress.check_cancel()?;
        let target = dst.join(rel);
        std::fs::create_dir_all(&target)
            .map_err(|e| AppError::Io(format!("Cannot create {}: {}", target.display(), e)))?;
    }

    for_each_file_parallel(&plan.root, progress, |path, size| {
        let rel = path.strip_prefix(&plan.root).unwrap_or(path);
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            progress.set_current(name);
        }
        copy_one(path, &dst.join(rel), size, progress)?;
        progress.file_done();
        Ok(())
    })
}

fn run_transfer(
    plans: &[SourcePlan],
    dest_dir: &Path,
    cut: bool,
    progress: &dyn Progress,
) -> AppResult<()> {
    if !dest_dir.is_dir() {
        return Err(AppError::InvalidPath(format!(
            "Not a folder: {}",
            dest_dir.display()
        )));
    }

    for plan in plans {
        progress.check_cancel()?;
        let src = plan.root.as_path();
        if !src.exists() {
            return Err(AppError::Io(format!("Source is gone: {}", src.display())));
        }
        assert_not_nested(src, dest_dir)?;

        let name = src
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| AppError::InvalidPath(format!("Bad source name: {}", src.display())))?;
        let dst = unique_destination(dest_dir, name);

        if cut {
            // Same-volume moves are a metadata operation, so the whole
            // subtree completes instantly; only a cross-volume move has
            // to fall back to copy + delete.
            match std::fs::rename(src, &dst) {
                Ok(()) => {
                    progress.set_current(name);
                    progress.add_completed(plan.files, plan.bytes);
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {}
                Err(e) => return Err(AppError::Io(format!("Cannot move {}: {}", name, e))),
            }
        }

        copy_source(plan, &dst, progress)?;

        if cut {
            // The bytes were already counted by the copy phase, so this
            // pass reports no further progress.
            delete_source_quietly(plan)?;
        }
    }
    Ok(())
}

fn delete_source_quietly(plan: &SourcePlan) -> AppResult<()> {
    let src = plan.root.as_path();
    let result = if plan.is_dir {
        std::fs::remove_dir_all(src)
    } else {
        std::fs::remove_file(src)
    };
    result.map_err(|e| AppError::Io(format!("Cannot remove {}: {}", src.display(), e)))
}

fn execute(
    kind: JobKind,
    plans: &[SourcePlan],
    dest_dir: &str,
    progress: &dyn Progress,
) -> AppResult<()> {
    match kind {
        JobKind::Delete => {
            for plan in plans {
                delete_source(plan, progress)?;
            }
            Ok(())
        }
        JobKind::Copy => run_transfer(plans, Path::new(dest_dir), false, progress),
        JobKind::Move => run_transfer(plans, Path::new(dest_dir), true, progress),
    }
}

/// Queue a job and return its id immediately; the work happens on a
/// dedicated OS thread so the caller (and the UI) is never held up.
///
/// Deliberately **not** `tokio::spawn_blocking`: this is invoked from a
/// synchronous Tauri command, where no reactor is installed, and a copy
/// can occupy its thread for minutes — which would starve the blocking
/// pool that every directory listing also depends on.
pub fn spawn<R: Runtime>(
    app: AppHandle<R>,
    registry: &OpsRegistry,
    kind: JobKind,
    sources: Vec<String>,
    dest_dir: String,
) -> String {
    let job = registry.create(kind, dest_dir.clone());
    let id = job.snapshot().id;
    let failure_app = app.clone();
    let failure_job = job.clone();

    let worker = move || {
        let reporter = Reporter::new(app, job);
        reporter.flush(true);

        let outcome = (|| -> AppResult<()> {
            let plans = scan_sources(&sources, &reporter)?;
            let total_files: u64 = plans.iter().map(|p| p.files).sum();
            let total_bytes: u64 = plans.iter().map(|p| p.bytes).sum();
            reporter.mutate(|s| {
                s.total_files = total_files;
                s.total_bytes = total_bytes;
                s.status = JobStatus::Running;
            });
            reporter.flush(true);
            execute(kind, &plans, &dest_dir, &reporter)
        })();

        reporter.sync_counters();
        reporter.mutate(|s| match &outcome {
            Ok(()) => {
                s.status = JobStatus::Completed;
                s.current = String::new();
                // A scan can undercount if the tree changed underneath us;
                // finishing at less than 100% looks like a stall.
                s.done_files = s.done_files.max(s.total_files);
                s.done_bytes = s.done_bytes.max(s.total_bytes);
            }
            Err(AppError::Cancelled) => s.status = JobStatus::Cancelled,
            Err(e) => {
                s.status = JobStatus::Failed;
                s.error = e.to_string();
            }
        });
        reporter.flush(true);
    };

    if let Err(e) = std::thread::Builder::new()
        .name(format!("fileop-{}", kind.label()))
        .spawn(worker)
    {
        // Surface the failure as a failed job instead of leaving a row
        // stuck at "Counting files…" forever.
        let snapshot = {
            let mut state = failure_job.state.lock().unwrap();
            state.status = JobStatus::Failed;
            state.error = format!("Cannot start worker thread: {}", e);
            state.clone()
        };
        log::error!("{}", snapshot.error);
        let _ = failure_app.emit("fileop:update", snapshot);
    }

    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts progress without needing a Tauri app handle.
    #[derive(Default)]
    struct Counter {
        bytes: AtomicU64,
        files: AtomicU64,
        cancel_after_files: Option<u64>,
    }

    impl Counter {
        fn files(&self) -> u64 {
            self.files.load(Ordering::Relaxed)
        }
        fn bytes(&self) -> u64 {
            self.bytes.load(Ordering::Relaxed)
        }
    }

    impl Progress for Counter {
        fn add_bytes(&self, n: u64) {
            self.bytes.fetch_add(n, Ordering::Relaxed);
        }
        fn file_done(&self) {
            self.files.fetch_add(1, Ordering::Relaxed);
        }
        fn set_current(&self, _name: &str) {}
        fn add_completed(&self, files: u64, bytes: u64) {
            self.files.fetch_add(files, Ordering::Relaxed);
            self.bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        fn is_cancelled(&self) -> bool {
            matches!(self.cancel_after_files, Some(limit) if self.files() >= limit)
        }
    }

    /// Unique temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("shuttle-files-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
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

    /// `root/tree/{a.txt, sub/b.txt}` — 2 files, 8 bytes.
    fn make_tree(root: &Path) -> PathBuf {
        let tree = root.join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("a.txt"), b"aaaa").unwrap();
        std::fs::write(tree.join("sub").join("b.txt"), b"bbbb").unwrap();
        tree
    }

    /// A wider tree, to exercise the parallel worker pool.
    fn make_wide_tree(root: &Path, dirs: usize, files_per_dir: usize) -> PathBuf {
        let tree = root.join("wide");
        for d in 0..dirs {
            let dir = tree.join(format!("d{}", d)).join("inner");
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..files_per_dir {
                std::fs::write(dir.join(format!("f{}.bin", f)), vec![b'x'; 16]).unwrap();
            }
        }
        tree
    }

    fn plan_for(path: &Path) -> SourcePlan {
        scan_source(path, &Counter::default()).unwrap()
    }

    #[test]
    fn scan_counts_files_and_collects_dirs() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let plan = plan_for(&tree);
        assert_eq!((plan.files, plan.bytes), (2, 8));
        assert_eq!(plan.dirs, vec![PathBuf::from("sub")]);
        assert!(plan.is_dir);
    }

    #[test]
    fn scan_of_a_single_file_skips_the_walk() {
        let tmp = TempDir::new();
        let file = tmp.path().join("solo.txt");
        std::fs::write(&file, b"12345").unwrap();
        let plan = plan_for(&file);
        assert!(!plan.is_dir);
        assert_eq!((plan.files, plan.bytes), (1, 5));
        assert!(plan.dirs.is_empty());
    }

    #[test]
    fn copy_recreates_the_whole_subtree() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let dst = tmp.path().join("copy");

        let counter = Counter::default();
        copy_source(&plan_for(&tree), &dst, &counter).unwrap();

        assert_eq!(std::fs::read(dst.join("a.txt")).unwrap(), b"aaaa");
        assert_eq!(
            std::fs::read(dst.join("sub").join("b.txt")).unwrap(),
            b"bbbb"
        );
        assert_eq!(counter.files(), 2);
        assert_eq!(counter.bytes(), 8);
        // The source must survive a copy.
        assert!(tree.join("a.txt").exists());
    }

    #[test]
    fn parallel_copy_moves_every_file_exactly_once() {
        let tmp = TempDir::new();
        let tree = make_wide_tree(tmp.path(), 12, 20);
        let dst = tmp.path().join("copy");

        let plan = plan_for(&tree);
        assert_eq!(plan.files, 240);

        let counter = Counter::default();
        copy_source(&plan, &dst, &counter).unwrap();

        assert_eq!(counter.files(), 240, "every file reported exactly once");
        assert_eq!(counter.bytes(), 240 * 16);
        assert_eq!(plan_for(&dst).files, 240, "destination has every file");
        assert_eq!(
            std::fs::read(dst.join("d5").join("inner").join("f7.bin")).unwrap(),
            vec![b'x'; 16]
        );
    }

    #[test]
    fn parallel_delete_removes_the_whole_subtree() {
        let tmp = TempDir::new();
        let tree = make_wide_tree(tmp.path(), 8, 10);

        let counter = Counter::default();
        delete_source(&plan_for(&tree), &counter).unwrap();

        assert!(!tree.exists(), "nothing should be left behind");
        assert_eq!(counter.files(), 80);
    }

    #[test]
    fn delete_clears_the_read_only_attribute() {
        let tmp = TempDir::new();
        let file = tmp.path().join("locked.txt");
        std::fs::write(&file, b"x").unwrap();
        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&file, perms).unwrap();

        delete_source(&plan_for(&file), &Counter::default()).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn paste_renames_instead_of_overwriting() {
        let tmp = TempDir::new();
        let src_dir = tmp.path().join("src");
        let dst_dir = tmp.path().join("dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&dst_dir).unwrap();
        std::fs::write(src_dir.join("note.txt"), b"new").unwrap();
        std::fs::write(dst_dir.join("note.txt"), b"original").unwrap();

        let plans = vec![plan_for(&src_dir.join("note.txt"))];
        run_transfer(&plans, &dst_dir, false, &Counter::default()).unwrap();

        assert_eq!(std::fs::read(dst_dir.join("note.txt")).unwrap(), b"original");
        assert_eq!(std::fs::read(dst_dir.join("note (2).txt")).unwrap(), b"new");
    }

    #[test]
    fn move_on_the_same_volume_removes_the_source() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let dst_dir = tmp.path().join("dst");
        std::fs::create_dir_all(&dst_dir).unwrap();

        let counter = Counter::default();
        run_transfer(&[plan_for(&tree)], &dst_dir, true, &counter).unwrap();

        assert!(!tree.exists(), "source should be gone after a move");
        assert!(dst_dir.join("tree").join("sub").join("b.txt").exists());
        // The rename fast path still has to report the full totals.
        assert_eq!(counter.files(), 2);
        assert_eq!(counter.bytes(), 8);
    }

    #[test]
    fn copying_a_folder_into_itself_is_rejected() {
        let tmp = TempDir::new();
        let tree = make_tree(tmp.path());
        let nested = tree.join("sub");

        let err = run_transfer(&[plan_for(&tree)], &nested, false, &Counter::default())
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidPath(_)), "got {:?}", err);
        // And nothing was written before the check fired.
        assert!(!nested.join("tree").exists());
    }

    #[test]
    fn cancellation_stops_the_copy() {
        let tmp = TempDir::new();
        let tree = make_wide_tree(tmp.path(), 10, 20);
        let dst = tmp.path().join("copy");

        let counter = Counter {
            cancel_after_files: Some(5),
            ..Default::default()
        };
        let err = copy_source(&plan_for(&tree), &dst, &counter).unwrap_err();
        assert!(matches!(err, AppError::Cancelled), "got {:?}", err);
        assert!(
            counter.files() < 200,
            "should stop early, copied {}",
            counter.files()
        );
    }

    #[test]
    fn a_large_file_reports_progress_while_copying() {
        let tmp = TempDir::new();
        let src = tmp.path().join("big.bin");
        let dst = tmp.path().join("big-copy.bin");
        // Comfortably past the chunk size used by both backends.
        let size = 12 << 20;
        std::fs::write(&src, vec![7u8; size]).unwrap();

        let counter = Counter::default();
        copy_one(&src, &dst, size as u64, &counter).unwrap();

        assert_eq!(std::fs::metadata(&dst).unwrap().len(), size as u64);
        assert_eq!(counter.bytes(), size as u64, "all bytes accounted for");
    }

    // --- Benchmarks ---------------------------------------------------------
    //
    // Not part of the normal suite (they write hundreds of megabytes).
    // Run with:
    //   cargo test --release bench_ -- --ignored --nocapture

    /// The straightforward recursion this engine replaced, kept as the
    /// baseline to measure against.
    fn naive_copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
        if src.is_dir() {
            std::fs::create_dir_all(dst)?;
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                naive_copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
            }
            Ok(())
        } else {
            std::fs::copy(src, dst).map(|_| ())
        }
    }

    fn naive_scan(path: &Path) -> (u64, u64) {
        let (mut files, mut bytes) = (0, 0);
        if path.is_dir() {
            if let Ok(rd) = std::fs::read_dir(path) {
                for entry in rd.flatten() {
                    let (f, b) = naive_scan(&entry.path());
                    files += f;
                    bytes += b;
                }
            }
        } else if let Ok(meta) = std::fs::metadata(path) {
            files += 1;
            bytes += meta.len();
        }
        (files, bytes)
    }

    fn seconds(f: impl FnOnce()) -> f64 {
        let start = Instant::now();
        f();
        start.elapsed().as_secs_f64()
    }

    #[test]
    #[ignore = "benchmark: writes hundreds of MB"]
    fn bench_many_small_files() {
        let tmp = TempDir::new();
        // 200 folders x 50 files x 8 KiB = 10 000 files, ~80 MiB.
        let dirs = 200;
        let per_dir = 50;
        let tree = tmp.path().join("bench");
        let payload = vec![b'x'; 8 << 10];
        for d in 0..dirs {
            let dir = tree.join(format!("d{:03}", d));
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..per_dir {
                std::fs::write(dir.join(format!("f{:03}.bin", f)), &payload).unwrap();
            }
        }
        let total = dirs * per_dir;

        let naive_scan_secs = {
            let mut result = (0, 0);
            let s = seconds(|| result = naive_scan(&tree));
            assert_eq!(result.0, total as u64);
            s
        };
        let plan = plan_for(&tree);
        let jwalk_scan_secs = seconds(|| {
            let p = plan_for(&tree);
            assert_eq!(p.files, total as u64);
        });

        let naive_copy_secs = {
            let dst = tmp.path().join("naive-copy");
            seconds(|| naive_copy_tree(&tree, &dst).unwrap())
        };
        let pool_copy_secs = {
            let dst = tmp.path().join("pool-copy");
            let counter = Counter::default();
            let s = seconds(|| copy_source(&plan, &dst, &counter).unwrap());
            assert_eq!(counter.files(), total as u64);
            s
        };

        let naive_delete_secs = {
            let dst = tmp.path().join("naive-copy");
            seconds(|| std::fs::remove_dir_all(&dst).unwrap())
        };
        let pool_delete_secs = {
            let dst = tmp.path().join("pool-copy");
            let target_plan = plan_for(&dst);
            seconds(|| delete_source(&target_plan, &Counter::default()).unwrap())
        };

        println!("\n{} files across {} folders, 8 KiB each", total, dirs);
        println!("  scan   : naive {naive_scan_secs:.3}s  ->  jwalk {jwalk_scan_secs:.3}s  ({:.2}x)", naive_scan_secs / jwalk_scan_secs);
        println!("  copy   : naive {naive_copy_secs:.3}s  ->  pool  {pool_copy_secs:.3}s  ({:.2}x)", naive_copy_secs / pool_copy_secs);
        println!("  delete : naive {naive_delete_secs:.3}s  ->  pool  {pool_delete_secs:.3}s  ({:.2}x)", naive_delete_secs / pool_delete_secs);
    }

    #[test]
    #[ignore = "benchmark: writes hundreds of MB"]
    fn bench_one_large_file() {
        let tmp = TempDir::new();
        let src = tmp.path().join("large.bin");
        let size = 512 << 20; // 512 MiB
        {
            use std::io::Write;
            let mut f = std::io::BufWriter::new(std::fs::File::create(&src).unwrap());
            let chunk = vec![b'z'; 1 << 20];
            for _ in 0..(size >> 20) {
                f.write_all(&chunk).unwrap();
            }
            f.flush().unwrap();
        }

        let naive_secs = {
            let dst = tmp.path().join("naive.bin");
            seconds(|| {
                std::fs::copy(&src, &dst).unwrap();
            })
        };
        let native_secs = {
            let dst = tmp.path().join("native.bin");
            let counter = Counter::default();
            let s = seconds(|| copy_one(&src, &dst, size as u64, &counter).unwrap());
            assert_eq!(counter.bytes(), size as u64);
            s
        };

        let mib = (size >> 20) as f64;
        println!("\n512 MiB single file");
        println!("  fs::copy      {naive_secs:.3}s  ({:.0} MiB/s)", mib / naive_secs);
        println!("  CopyFile2     {native_secs:.3}s  ({:.0} MiB/s, with progress + cancel)", mib / native_secs);
    }
}
