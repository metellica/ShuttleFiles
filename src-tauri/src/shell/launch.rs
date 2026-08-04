//! Handing a file or folder to the program that opens it.
//!
//! The obvious implementation — the opener plugin — starts the handler
//! without naming a working directory, so the handler inherits
//! ShuttleFiles' own, which is wherever the app happened to be launched
//! from. Anything that resolves paths relative to itself then breaks:
//! a `StartAll.bat` that calls `.\env.bat` reports that `env.bat` does
//! not exist, while the same double click in Explorer works. Explorer
//! starts the handler in the item's own folder, and so does this module.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Where the handler should start: the folder holding `path`, or `path`
/// itself when it is a folder. `None` for a drive root, whose parent
/// does not exist — the shell then falls back to its own default.
fn working_dir(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    (!dir.as_os_str().is_empty()).then_some(dir)
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use crate::error::{AppError, AppResult};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn open(path: &Path, program: Option<&str>) -> AppResult<()> {
        let target = path.to_string_lossy().to_string();
        let dir = super::working_dir(path).map(|d| wide(&d.to_string_lossy()));

        // With a program, the file becomes its command line; the quotes
        // keep a path with spaces a single argument.
        let file = wide(program.unwrap_or(&target));
        let params = program.map(|_| wide(&format!("\"{}\"", target)));

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            // No message loop runs on this thread, so the shell must not
            // hand the request off asynchronously.
            fMask: SEE_MASK_NOASYNC,
            nShow: SW_SHOWNORMAL.0,
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: params.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            lpDirectory: dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
            ..Default::default()
        };

        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| match program {
            Some(p) => AppError::Io(format!("Cannot start {}: {}", p, e.message())),
            None => AppError::Io(format!("Cannot open {}: {}", target, e.message())),
        })
    }
}

#[cfg(not(windows))]
mod imp {
    //! Non-Windows builds delegate to the platform opener, which takes
    //! no working directory; the relative-path problem this module
    //! exists for is a Windows one.

    use std::path::Path;

    use crate::error::{AppError, AppResult};

    pub fn open(path: &Path, program: Option<&str>) -> AppResult<()> {
        let dir = super::working_dir(path);
        let mut command = match program {
            Some(p) => {
                let mut c = std::process::Command::new(p);
                c.arg(path);
                c
            }
            None => {
                let mut c = std::process::Command::new("xdg-open");
                c.arg(path);
                c
            }
        };
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        command
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::Io(format!("Cannot open {}: {}", path.display(), e)))
    }
}

/// Opens `path` with `program`, or with the system default when
/// `program` is empty. Blocking: the shell may take a moment.
pub fn open(path: &str, program: Option<&str>) -> AppResult<()> {
    let program = program.map(str::trim).filter(|p| !p.is_empty());
    let target = Path::new(path);
    // Reporting a missing file here beats the shell's own wording, which
    // names the handler rather than the file the user clicked.
    if !target.exists() {
        return Err(AppError::InvalidPath(format!("No such file: {}", path)));
    }
    imp::open(target, program)
}

/// Command-facing wrapper; keeps the shell call off the IPC thread.
pub async fn open_async(path: String, program: Option<String>) -> AppResult<()> {
    tokio::task::spawn_blocking(move || open(&path, program.as_deref()))
        .await
        .map_err(|e| AppError::Io(format!("Open task failed: {}", e)))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_starts_in_its_own_folder() {
        let dir = std::env::temp_dir().join("shuttlefiles-launch-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("StartAll.bat");
        assert_eq!(working_dir(&file), Some(dir));
    }

    #[test]
    fn folder_starts_in_itself() {
        let dir = std::env::temp_dir();
        assert_eq!(working_dir(&dir), Some(dir));
    }

    #[test]
    fn missing_path_is_rejected() {
        let missing = std::env::temp_dir().join("shuttlefiles-no-such-file.bat");
        assert!(open(&missing.to_string_lossy(), None).is_err());
    }

    /// The regression this module exists for: a script that calls a
    /// sibling by relative path. Ignored by default because it pops up a
    /// console window and needs a desktop session.
    /// `cargo test relative_paths_resolve -- --ignored --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore = "launches a console window"]
    fn relative_paths_resolve_against_the_scripts_folder() {
        let dir = std::env::temp_dir().join("shuttlefiles-launch-relative");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("done.txt");
        std::fs::write(dir.join("child.bat"), "echo ok> \"%~dp0done.txt\"\r\n").unwrap();
        std::fs::write(dir.join("StartAll.bat"), "@echo off\r\ncall .\\child.bat\r\n").unwrap();

        open(&dir.join("StartAll.bat").to_string_lossy(), None).expect("launch");

        for _ in 0..100 {
            if marker.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("child.bat never ran: the working directory was not the script's folder");
    }
}
