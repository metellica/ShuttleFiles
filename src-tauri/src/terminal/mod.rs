//! Local PTY-based terminals embedded in the application.
//!
//! Each terminal is a pseudo-terminal running a shell process (cmd,
//! PowerShell, VS Developer Prompt, Git Bash, etc.). Output is streamed
//! to the frontend as base64 `terminal:data` events; input/resize/close
//! come back through Tauri commands.
//!
//! The design mirrors ShuttleSFTP's terminal manager but targets local
//! PTYs via `portable-pty` instead of SSH channels.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::Emitter;
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::shell::terminal as detection;

#[cfg(windows)]
struct ProcessJob {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl ProcessJob {
    fn assign(pid: u32) -> Result<Self, String> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())
                .map_err(|e| format!("Cannot create terminal job: {}", e))?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) {
                let _ = CloseHandle(job);
                return Err(format!("Cannot configure terminal job: {}", e));
            }

            let process = match OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) {
                Ok(process) => process,
                Err(e) => {
                    let _ = CloseHandle(job);
                    return Err(format!("Cannot open terminal process: {}", e));
                }
            };
            let assigned = AssignProcessToJobObject(job, process);
            let _ = CloseHandle(process);
            if let Err(e) = assigned {
                let _ = CloseHandle(job);
                return Err(format!("Cannot assign terminal process to job: {}", e));
            }
            Ok(Self { handle: job })
        }
    }

    fn terminate(&self) {
        use windows::Win32::System::JobObjects::TerminateJobObject;
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(not(windows))]
struct ProcessJob;

#[cfg(not(windows))]
impl ProcessJob {
    fn assign(_pid: u32) -> Result<Self, String> {
        Ok(Self)
    }

    fn terminate(&self) {}
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn b64_decode(data: &str) -> AppResult<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| AppError::Io(format!("Invalid base64: {}", e)))
}

/// Public decode for use from the commands module.
pub fn b64_decode_pub(data: &str) -> AppResult<Vec<u8>> {
    b64_decode(data)
}

/// Forward terminal output to the frontend.
fn emit_data(app: &tauri::AppHandle, id: &str, data: &[u8]) {
    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DataEvent<'a> {
        id: &'a str,
        data: String,
    }
    let _ = app.emit(
        "terminal:data",
        DataEvent {
            id,
            data: b64(data),
        },
    );
}

fn emit_exit(app: &tauri::AppHandle, id: &str) {
    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExitEvent<'a> {
        id: &'a str,
    }
    let _ = app.emit("terminal:exit", ExitEvent { id });
}

/// Control messages sent to a running terminal's pump task.
enum TermCmd {
    Input(Vec<u8>),
    Resize(u16, u16),
    Close,
}

/// A registered terminal entry.
struct LiveTerm {
    token: String,
    tx: mpsc::UnboundedSender<TermCmd>,
    /// The command receiver, held here until `open` claims it.
    rx: Option<mpsc::UnboundedReceiver<TermCmd>>,
}

type TermMap = Arc<std::sync::Mutex<HashMap<String, LiveTerm>>>;

fn lock_terms(m: &TermMap) -> std::sync::MutexGuard<'_, HashMap<String, LiveTerm>> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Manages local PTY terminals.
#[derive(Default)]
pub struct TerminalManager {
    terms: TermMap,
}

/// A claimed reservation, dropped if open fails.
pub struct TerminalSlot {
    pub id: String,
    pub token: String,
    rx: Option<mpsc::UnboundedReceiver<TermCmd>>,
    terms: TermMap,
    started: bool,
}

impl TerminalSlot {
    fn cancel(&mut self) {
        let mut reg = lock_terms(&self.terms);
        if let Some(term) = reg.get(&self.id).filter(|term| term.token == self.token) {
            let _ = term.tx.send(TermCmd::Close);
            reg.remove(&self.id);
        }
    }
}

impl Drop for TerminalSlot {
    fn drop(&mut self) {
        if !self.started {
            self.cancel();
        }
    }
}

impl TerminalManager {
    /// Reserve an id. Returns a token that must accompany all further commands.
    pub fn reserve(&self, terminal_id: &str) -> AppResult<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut reg = lock_terms(&self.terms);
        if reg.contains_key(terminal_id) {
            return Err(AppError::Io(format!(
                "Terminal {} already exists",
                terminal_id
            )));
        }
        let token = uuid::Uuid::new_v4().to_string();
        reg.insert(
            terminal_id.to_string(),
            LiveTerm {
                token: token.clone(),
                tx,
                rx: Some(rx),
            },
        );
        // Timeout: clean up if never claimed.
        let terms = self.terms.clone();
        let id = terminal_id.to_string();
        let t = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            let mut reg = lock_terms(&terms);
            if reg
                .get(&id)
                .is_some_and(|entry| entry.token == t && entry.rx.is_some())
            {
                reg.remove(&id);
            }
        });
        Ok(token)
    }

    /// Claim the reservation, taking the command receiver.
    pub fn claim(&self, terminal_id: &str, token: &str) -> AppResult<TerminalSlot> {
        let mut reg = lock_terms(&self.terms);
        let entry = reg
            .get_mut(terminal_id)
            .filter(|e| e.token == token)
            .ok_or_else(|| AppError::Io("Terminal reservation is closed".into()))?;
        let rx = entry
            .rx
            .take()
            .ok_or_else(|| AppError::Io("Terminal reservation was already used".into()))?;
        Ok(TerminalSlot {
            id: terminal_id.to_string(),
            token: token.to_string(),
            rx: Some(rx),
            terms: self.terms.clone(),
            started: false,
        })
    }

    /// Open a local PTY terminal of the given type at `cwd`.
    pub fn open(
        &self,
        app: tauri::AppHandle,
        mut slot: TerminalSlot,
        shell_id: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> AppResult<()> {
        let (cmd, init) = build_command(shell_id, cwd)?;

        let terminal_id = slot.id.clone();
        let token = slot.token.clone();
        let terms = self.terms.clone();
        let rx = slot.rx.take().unwrap();

        // Use a channel to report startup success/failure back.
        let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        // Do ALL PTY operations on a dedicated thread to avoid any
        // interaction with the tokio runtime or Tauri IPC threads.
        std::thread::spawn(move || {
            let startup = (|| -> Result<(), String> {
                let pty_system = native_pty_system();
                let pair = pty_system
                    .openpty(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| format!("Cannot open PTY: {}", e))?;

                let child = pair
                    .slave
                    .spawn_command(cmd)
                    .map_err(|e| format!("Cannot spawn shell: {}", e))?;
                let job = child
                    .process_id()
                    .and_then(|pid| match ProcessJob::assign(pid) {
                        Ok(job) => Some(job),
                        Err(e) => {
                            log::warn!("{}", e);
                            None
                        }
                    });
                drop(pair.slave);

                let reader = pair
                    .master
                    .try_clone_reader()
                    .map_err(|e| format!("Cannot get PTY reader: {}", e))?;
                let writer = pair
                    .master
                    .take_writer()
                    .map_err(|e| format!("Cannot get PTY writer: {}", e))?;

                // Signal success before entering the pump loop.
                let _ = result_tx.send(Ok(()));

                run_terminal(
                    app,
                    terminal_id,
                    token,
                    terms,
                    pair.master,
                    reader,
                    writer,
                    child,
                    job,
                    rx,
                    init,
                );
                Ok(())
            })();

            if let Err(e) = startup {
                let _ = result_tx.send(Err(e));
            }
        });

        // Wait for the thread to report startup status.
        match result_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => {
                slot.started = true;
                Ok(())
            }
            Ok(Err(e)) => {
                slot.cancel();
                Err(AppError::Io(e))
            }
            Err(_) => {
                slot.cancel();
                Err(AppError::Io("Terminal startup timed out".into()))
            }
        }
    }

    /// Send input data to a terminal.
    pub fn input(&self, terminal_id: &str, token: &str, data: Vec<u8>) -> AppResult<()> {
        let reg = lock_terms(&self.terms);
        let entry = reg
            .get(terminal_id)
            .filter(|e| e.token == token)
            .ok_or_else(|| AppError::Io("Terminal not found".into()))?;
        entry
            .tx
            .send(TermCmd::Input(data))
            .map_err(|_| AppError::Io("Terminal closed".into()))
    }

    /// Resize a terminal.
    pub fn resize(&self, terminal_id: &str, token: &str, cols: u16, rows: u16) -> AppResult<()> {
        let reg = lock_terms(&self.terms);
        let entry = reg
            .get(terminal_id)
            .filter(|e| e.token == token)
            .ok_or_else(|| AppError::Io("Terminal not found".into()))?;
        entry
            .tx
            .send(TermCmd::Resize(cols, rows))
            .map_err(|_| AppError::Io("Terminal closed".into()))
    }

    /// Close a terminal.
    pub fn close(&self, terminal_id: &str, token: &str) -> AppResult<()> {
        let reg = lock_terms(&self.terms);
        if let Some(entry) = reg.get(terminal_id).filter(|e| e.token == token) {
            let _ = entry.tx.send(TermCmd::Close);
        }
        Ok(())
    }
}

/// Build the PTY command for a given shell type and working directory.
/// Returns (command, optional_init_input) — the init input is sent to stdin after startup.
fn build_command(shell_id: &str, cwd: &str) -> AppResult<(CommandBuilder, Option<String>)> {
    let (mut cmd, init) = match shell_id {
        "cmd" => (CommandBuilder::new("cmd.exe"), None),
        "powershell" => {
            let exe = if which_exists("pwsh.exe") {
                "pwsh.exe"
            } else {
                "powershell.exe"
            };
            let mut c = CommandBuilder::new(exe);
            c.arg("-NoExit");
            (c, None)
        }
        "git-bash" => {
            let exe = detection::find_git_bash_path()
                .ok_or_else(|| AppError::Config("Git Bash is not installed".into()))?;
            // Use bash.exe inside Git's usr/bin/, not git-bash.exe (which opens its own mintty window).
            let bash_exe = exe
                .parent()
                .unwrap_or(exe.as_path())
                .join("usr\\bin\\bash.exe");
            let actual = if bash_exe.is_file() { bash_exe } else { exe };
            let mut c = CommandBuilder::new(actual.to_string_lossy().to_string());
            c.args(["--login", "-i"]);
            (c, None)
        }
        other => {
            // Visual Studio terminals: "<vsid>-cmd" or "<vsid>-ps"
            if let Some(suffix) = other.strip_suffix("-cmd") {
                if let Some(vcvars) = find_vcvars(suffix) {
                    let c = CommandBuilder::new("cmd.exe");
                    // Send the VsDevCmd.bat call as stdin after shell starts.
                    let init = format!("call \"{}\"\r\n", vcvars.display());
                    (c, Some(init))
                } else {
                    return Err(AppError::Config(format!("Unknown terminal: {}", other)));
                }
            } else if let Some(suffix) = other.strip_suffix("-ps") {
                if let Some(vcvars) = find_vcvars(suffix) {
                    let exe = if which_exists("pwsh.exe") {
                        "pwsh.exe"
                    } else {
                        "powershell.exe"
                    };
                    let mut c = CommandBuilder::new(exe);
                    c.arg("-NoExit");
                    let init = format!(
                        "{}\r\n",
                        detection::powershell_vs_environment_command(&vcvars)
                    );
                    (c, Some(init))
                } else {
                    return Err(AppError::Config(format!("Unknown terminal: {}", other)));
                }
            } else {
                return Err(AppError::Config(format!("Unknown terminal: {}", other)));
            }
        }
    };
    cmd.cwd(cwd);
    Ok((cmd, init))
}

/// Find the VsDevCmd.bat path for a given VS installation id prefix.
fn find_vcvars(vs_id_prefix: &str) -> Option<std::path::PathBuf> {
    detection::find_vcvars_for(vs_id_prefix)
}

fn which_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// The terminal pump: reads output, processes input/resize/close commands.
fn run_terminal(
    app: tauri::AppHandle,
    terminal_id: String,
    token: String,
    terms: TermMap,
    master: Box<dyn portable_pty::MasterPty + Send>,
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    job: Option<ProcessJob>,
    rx: mpsc::UnboundedReceiver<TermCmd>,
    init: Option<String>,
) {
    let exited = Arc::new(AtomicBool::new(false));
    let closing = Arc::new(AtomicBool::new(false));

    // Reader thread: reads PTY output and emits to frontend.
    let app2 = app.clone();
    let id2 = terminal_id.clone();
    let exited2 = exited.clone();
    let closing2 = closing.clone();
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) if !closing2.load(Ordering::SeqCst) => emit_data(&app2, &id2, &buf[..n]),
                Ok(_) => break,
            }
        }
        exited2.store(true, Ordering::SeqCst);
    });

    // Send initialization command (e.g. VsDevCmd.bat) after a brief delay.
    if let Some(init_cmd) = init {
        std::thread::sleep(Duration::from_millis(200));
        let _ = writer.write_all(init_cmd.as_bytes());
    }

    // Command pump: blocking loop receiving commands.
    let mut rx = rx;
    loop {
        // Non-blocking try_recv with a small sleep to avoid busy-wait.
        match rx.try_recv() {
            Ok(TermCmd::Input(data)) => {
                if writer.write_all(&data).is_err() {
                    break;
                }
            }
            Ok(TermCmd::Resize(cols, rows)) => {
                let _ = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            Ok(TermCmd::Close) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
            Err(mpsc::error::TryRecvError::Empty) => {
                if exited.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    // Stop the full process tree, close ConPTY handles, and never block
    // registry cleanup on a reader retained by a descendant.
    closing.store(true, Ordering::SeqCst);
    if let Some(job) = &job {
        job.terminate();
    }
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(master);
    drop(job);
    drop(reader_handle);

    // Remove from registry and emit exit.
    {
        let mut reg = lock_terms(&terms);
        if reg.get(&terminal_id).is_some_and(|e| e.token == token) {
            reg.remove(&terminal_id);
        }
    }
    emit_exit(&app, &terminal_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_works_without_a_tokio_runtime() {
        let terminals = TerminalManager::default();
        let token = terminals.reserve("test-terminal").expect("reserve");
        assert!(!token.is_empty());
    }

    #[test]
    fn dropping_a_claimed_slot_releases_its_id() {
        let terminals = TerminalManager::default();
        let token = terminals.reserve("claimed-terminal").expect("reserve");
        drop(terminals.claim("claimed-terminal", &token).expect("claim"));
        terminals
            .reserve("claimed-terminal")
            .expect("reserve the released id");
    }
}
