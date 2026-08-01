//! Bridge to the out-of-process shell context menu helper.
//!
//! The helper (`shellmenu.exe`) is spawned per menu and exits with it,
//! so third-party extension DLLs are loaded into a throw-away process
//! and never into the browser.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Extensions occasionally hang; without a cap the browser would show a
/// menu entry that never resolves.
const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HelperRequest<'a> {
    mode: &'a str,
    paths: &'a [String],
    x: i32,
    y: i32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellMenuItem {
    pub id: Option<u32>,
    pub label: String,
    pub verb: String,
    pub separator: bool,
    pub enabled: bool,
    pub default: bool,
    /// Whether the shell attached a submenu. `children` can still be
    /// empty: several extensions build their submenu lazily, on the
    /// `WM_INITMENUPOPUP` that only arrives once the menu is shown.
    #[serde(default)]
    pub has_submenu: bool,
    pub children: Vec<ShellMenuItem>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperResponse {
    ok: bool,
    #[serde(default)]
    items: Vec<ShellMenuItem>,
    #[serde(default)]
    invoked: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellMenuResult {
    pub items: Vec<ShellMenuItem>,
    /// Verb the user picked; empty when the menu was dismissed.
    pub invoked: String,
}

/// Locate `shellmenu.exe`: next to the running binary during
/// development, or in the bundle's resource directory once installed.
fn helper_path(resource_dir: Option<PathBuf>) -> AppResult<PathBuf> {
    let name = if cfg!(windows) {
        "shellmenu.exe"
    } else {
        "shellmenu"
    };

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Development: cargo puts both binaries side by side.
            candidates.push(dir.join(name));
        }
    }
    if let Some(dir) = resource_dir {
        // Installed: staged into the bundle by `npm run build:helper`.
        candidates.push(dir.join("binaries").join(name));
        candidates.push(dir.join(name));
    }

    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| {
            AppError::Io(format!(
                "Cannot find {} (looked in: {})",
                name,
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

fn run_helper(
    resource_dir: Option<PathBuf>,
    mode: &str,
    paths: &[String],
    x: i32,
    y: i32,
) -> AppResult<ShellMenuResult> {
    if paths.is_empty() {
        return Err(AppError::InvalidPath("Nothing selected".into()));
    }
    let exe = helper_path(resource_dir)?;

    let mut child = Command::new(&exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Io(format!("Cannot start shell menu helper: {}", e)))?;

    // The helper has to be allowed to take the foreground, otherwise its
    // popup would not dismiss when the user clicks away.
    #[cfg(windows)]
    allow_foreground(child.id());

    let request = serde_json::to_vec(&HelperRequest { mode, paths, x, y })?;
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::Io("Helper stdin unavailable".into()))?
        .write_all(&request)
        .map_err(|e| AppError::Io(format!("Cannot send request to helper: {}", e)))?;

    // `show` blocks for as long as the menu is open, which is entirely
    // up to the user; only the non-interactive `list` gets a deadline.
    if mode == "list" {
        let deadline = std::time::Instant::now() + LIST_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    return Err(AppError::Io(
                        "A shell extension did not respond in time".into(),
                    ));
                }
                Err(e) => return Err(AppError::Io(format!("Helper failed: {}", e))),
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Io(format!("Helper failed: {}", e)))?;

    let response: HelperResponse = serde_json::from_slice(&output.stdout).map_err(|e| {
        AppError::Io(format!(
            "Bad response from helper ({}): {}",
            e,
            String::from_utf8_lossy(&output.stdout).chars().take(200).collect::<String>()
        ))
    })?;

    if !response.ok {
        return Err(AppError::Io(response.error));
    }
    Ok(ShellMenuResult {
        items: response.items,
        invoked: response.invoked,
    })
}

/// Grant the helper the right to call `SetForegroundWindow`.
#[cfg(windows)]
fn allow_foreground(pid: u32) {
    // Best effort: without it the menu still opens, it just may not
    // dismiss cleanly on an outside click.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid).ok();
    }
}

/// Report what the shell would show, without displaying anything.
/// Used by tests and diagnostics rather than the normal menu path.
pub fn list(resource_dir: Option<PathBuf>, paths: &[String]) -> AppResult<ShellMenuResult> {
    run_helper(resource_dir, "list", paths, 0, 0)
}

/// Display the native shell menu at screen coordinates and run whatever
/// the user picks.
pub fn show(
    resource_dir: Option<PathBuf>,
    paths: &[String],
    x: i32,
    y: i32,
) -> AppResult<ShellMenuResult> {
    run_helper(resource_dir, "show", paths, x, y)
}
