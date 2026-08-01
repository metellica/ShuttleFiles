use tauri::{AppHandle, Runtime, State};

use crate::error::{AppError, AppResult};
use crate::ops::{engine, JobKind, JobState, OpsRegistry};

/// Queue a copy/move/delete and return its job id immediately. Progress
/// arrives as `fileop:update` events; the UI stays usable throughout.
///
/// Generic over the runtime so the whole command can be exercised
/// against Tauri's mock runtime in tests.
#[tauri::command]
pub fn start_operation<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, OpsRegistry>,
    kind: JobKind,
    sources: Vec<String>,
    dest_dir: Option<String>,
) -> AppResult<String> {
    if sources.is_empty() {
        return Err(AppError::InvalidPath("Nothing to process".into()));
    }
    let dest_dir = match kind {
        JobKind::Delete => String::new(),
        _ => dest_dir
            .filter(|d| !d.is_empty())
            .ok_or_else(|| AppError::InvalidPath("No destination folder".into()))?,
    };
    Ok(engine::spawn(app, &registry, kind, sources, dest_dir))
}

#[tauri::command]
pub fn cancel_operation(registry: State<'_, OpsRegistry>, id: String) {
    if let Some(job) = registry.get(&id) {
        job.cancel();
    }
}

#[tauri::command]
pub fn list_operations(registry: State<'_, OpsRegistry>) -> Vec<JobState> {
    registry.list()
}

#[tauri::command]
pub fn clear_finished_operations(registry: State<'_, OpsRegistry>) {
    registry.clear_finished();
}
