use std::path::Path;

use tauri::State;

use crate::error::AppResult;
use crate::shell::terminal::{self, TerminalEntry};
use crate::terminal::TerminalManager;

/// Returns detected terminal environments (cached after first call).
#[tauri::command]
pub fn list_terminals() -> Vec<TerminalEntry> {
    terminal::list().to_vec()
}

/// Opens an external (non-embedded) terminal by id in the given directory.
#[tauri::command]
pub async fn open_terminal(id: String, cwd: String) -> AppResult<()> {
    let path = cwd.clone();
    tokio::task::spawn_blocking(move || terminal::open(&id, Path::new(&path)))
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Terminal task failed: {}", e)))?
}

// --- Embedded PTY terminal commands ---

/// Reserve an embedded terminal id. Returns an attempt-specific token.
#[tauri::command]
pub fn terminal_reserve(
    terminal_id: String,
    terminals: State<'_, TerminalManager>,
) -> AppResult<String> {
    terminals.reserve(&terminal_id)
}

/// Open a PTY terminal of the given shell type at the given cwd.
#[tauri::command]
pub async fn terminal_open(
    terminal_id: String,
    terminal_token: String,
    shell_id: String,
    cwd: String,
    cols: u16,
    rows: u16,
    app: tauri::AppHandle,
    terminals: State<'_, TerminalManager>,
) -> AppResult<()> {
    let slot = terminals.claim(&terminal_id, &terminal_token)?;
    terminals.open(app, slot, &shell_id, &cwd, cols, rows)
}

/// Feed keyboard input (base64-encoded) to an embedded terminal.
#[tauri::command]
pub fn terminal_input(
    terminal_id: String,
    terminal_token: String,
    data: String,
    terminals: State<'_, TerminalManager>,
) -> AppResult<()> {
    let bytes = crate::terminal::b64_decode_pub(&data)?;
    terminals.input(&terminal_id, &terminal_token, bytes)
}

/// Resize an embedded terminal.
#[tauri::command]
pub fn terminal_resize(
    terminal_id: String,
    terminal_token: String,
    cols: u16,
    rows: u16,
    terminals: State<'_, TerminalManager>,
) -> AppResult<()> {
    terminals.resize(&terminal_id, &terminal_token, cols, rows)
}

/// Close an embedded terminal.
#[tauri::command]
pub fn terminal_close(
    terminal_id: String,
    terminal_token: String,
    terminals: State<'_, TerminalManager>,
) -> AppResult<()> {
    terminals.close(&terminal_id, &terminal_token)
}
