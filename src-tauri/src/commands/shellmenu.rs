use tauri::{AppHandle, Manager, Runtime};

use crate::error::AppResult;
use crate::shell::menu::{self, ShellMenuResult};

fn resource_dir<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().resource_dir().ok()
}

/// Display the native shell context menu — including every registered
/// third-party extension — at the given screen coordinates, and run the
/// command the user picks.
///
/// Blocking on purpose: the helper's popup owns the interaction until it
/// closes. It runs on Tauri's async command pool, so the UI thread and
/// any background file operations are unaffected.
#[tauri::command]
pub async fn shell_menu_show<R: Runtime>(
    app: AppHandle<R>,
    paths: Vec<String>,
    x: i32,
    y: i32,
) -> AppResult<ShellMenuResult> {
    let dir = resource_dir(&app);
    tokio::task::spawn_blocking(move || menu::show(dir, &paths, x, y))
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Menu task failed: {}", e)))?
}

/// Report what the shell menu would contain without showing it. Used to
/// decide whether the "More options" entry is worth offering, and by the
/// integration tests.
#[tauri::command]
pub async fn shell_menu_list<R: Runtime>(
    app: AppHandle<R>,
    paths: Vec<String>,
) -> AppResult<ShellMenuResult> {
    let dir = resource_dir(&app);
    tokio::task::spawn_blocking(move || menu::list(dir, &paths))
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Menu task failed: {}", e)))?
}
