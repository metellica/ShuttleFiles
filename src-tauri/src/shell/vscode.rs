//! Handing a selection to Visual Studio Code.
//!
//! VS Code is the one program worth a menu entry of its own: it opens a
//! folder as a project and a pile of files as tabs in one window, which
//! is neither what the association does nor what the text-editor setting
//! is for. The entry only appears when the editor is actually installed,
//! so the menu never offers something that cannot happen.
//!
//! `code` on PATH is a `.cmd` wrapper, which a plain spawn cannot run
//! and which flashes a console window when it can; the real executable
//! is what gets started instead.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{AppError, AppResult};

/// Stable before Insiders: someone with both installed means the one
/// they call "VS Code".
const EDITIONS: &[(&str, &str)] = &[
    ("Code.exe", "Microsoft VS Code"),
    ("Code - Insiders.exe", "Microsoft VS Code Insiders"),
];

/// Cached: the answer cannot change while the app runs, and the context
/// menu asks on every right click.
static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn locate() -> Option<&'static Path> {
    FOUND.get_or_init(imp::find).as_deref()
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };

    use super::EDITIONS;

    /// Where the installer records the executable, whichever drive and
    /// user profile it landed on.
    const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The default value of `App Paths\<exe>`, which both the per-user
    /// and the system-wide installer write.
    fn registered(exe: &str) -> Option<PathBuf> {
        let key = wide(&format!("{}{}", APP_PATHS, exe));

        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let mut bytes: u32 = 0;
            let read = |data: Option<*mut std::ffi::c_void>, bytes: &mut u32| unsafe {
                RegGetValueW(
                    root,
                    PCWSTR(key.as_ptr()),
                    PCWSTR::null(),
                    RRF_RT_REG_SZ,
                    None,
                    data,
                    Some(bytes),
                )
            };

            if read(None, &mut bytes) != ERROR_SUCCESS || bytes < 4 {
                continue;
            }
            let mut buffer = vec![0u16; bytes as usize / 2];
            if read(Some(buffer.as_mut_ptr().cast()), &mut bytes) != ERROR_SUCCESS {
                continue;
            }
            // The value is NUL terminated, and the terminator is part of
            // the returned length.
            let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..end]));
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    /// Fallback for an installation the registry does not know about —
    /// a portable copy dropped in place, or a profile the installer did
    /// not get to finish writing to.
    fn install_roots() -> Vec<PathBuf> {
        ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
            .flat_map(|root| {
                // The per-user installer nests one level deeper.
                [root.join("Programs"), root]
            })
            .collect()
    }

    pub fn find() -> Option<PathBuf> {
        for (exe, _) in EDITIONS {
            if let Some(path) = registered(exe) {
                return Some(path);
            }
        }
        for root in install_roots() {
            for (exe, folder) in EDITIONS {
                let path = root.join(folder).join(exe);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        None
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    /// No registry to ask, so the launcher on PATH is the answer.
    pub fn find() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("code"))
            .find(|candidate| candidate.is_file())
    }
}

/// Variables a VS Code terminal exports into everything it starts. Left
/// in place they turn a fresh `Code.exe` into a plain Node process: it
/// exits without a window, without an error, and the path is never
/// opened — which is exactly what a user launching ShuttleFiles from
/// that terminal would see.
fn is_inherited_electron_var(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("VSCODE_") || name == "ELECTRON_RUN_AS_NODE" || name == "NODE_OPTIONS"
}

/// Drops those variables from ShuttleFiles' own environment, so nothing
/// it starts inherits them either. [`open`] scrubs its own child, but
/// the editor named in Settings is launched through the shell, which
/// passes this environment on and gives no way to change it. They mean
/// nothing to this app, so losing them costs it nothing.
///
/// Call before any other thread exists: the environment is process-wide.
pub fn purge_inherited_vars() {
    let doomed: Vec<_> = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| is_inherited_electron_var(&key.to_string_lossy()))
        .collect();
    for key in doomed {
        std::env::remove_var(key);
    }
}

/// Opens everything in `paths` in one VS Code window.
pub fn open(paths: &[String]) -> AppResult<()> {
    if paths.is_empty() {
        return Err(AppError::InvalidPath("Nothing selected".into()));
    }
    // An entry that is not a real path — the virtual root standing for
    // "This PC", say — would reach the editor as an empty argument and
    // silently open nothing.
    if let Some(bad) = paths.iter().find(|p| !Path::new(p).exists()) {
        return Err(AppError::InvalidPath(if bad.trim().is_empty() {
            "Not a folder Visual Studio Code can open".into()
        } else {
            format!("No such file or folder: {}", bad)
        }));
    }
    let exe =
        locate().ok_or_else(|| AppError::Config("Visual Studio Code is not installed".into()))?;

    let mut command = std::process::Command::new(exe);
    command.args(paths);
    for (key, _) in std::env::vars_os() {
        if is_inherited_electron_var(&key.to_string_lossy()) {
            command.env_remove(&key);
        }
    }
    // The app has no console, so the handles it would pass on are not
    // ones a detached editor should inherit.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Start where the user was looking, so a terminal opened inside a
    // fresh window lands in the same folder.
    if let Some(dir) = crate::shell::launch::working_dir(Path::new(&paths[0])) {
        command.current_dir(dir);
    }
    log::info!("opening {:?} with {}", paths, exe.display());
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::Io(format!("Cannot start {}: {}", exe.display(), e)))
}

/// Command-facing wrapper; keeps the spawn off the IPC thread.
pub async fn open_async(paths: Vec<String>) -> AppResult<()> {
    tokio::task::spawn_blocking(move || open(&paths))
        .await
        .map_err(|e| AppError::Io(format!("Open task failed: {}", e)))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_selected_is_rejected() {
        assert!(open(&[]).is_err());
    }

    /// "This PC" is not a folder any editor can open, and it reaches
    /// here as an empty string.
    #[test]
    fn the_virtual_root_is_rejected() {
        assert!(open(&[String::new()]).is_err());
    }

    #[test]
    fn a_path_that_does_not_exist_is_rejected() {
        let ghost = std::env::temp_dir().join("shuttlefiles-vscode-ghost.txt");
        assert!(open(&[ghost.to_string_lossy().to_string()]).is_err());
    }

    #[test]
    fn a_vs_code_terminals_variables_are_recognised() {
        for name in [
            "ELECTRON_RUN_AS_NODE",
            "VSCODE_CWD",
            "VSCODE_IPC_HOOK_CLI",
            "vscode_nls_config",
            "NODE_OPTIONS",
        ] {
            assert!(is_inherited_electron_var(name), "{} should be stripped", name);
        }
        for name in ["PATH", "TEMP", "VSCODEISH", "ELECTRON"] {
            assert!(!is_inherited_electron_var(name), "{} should be kept", name);
        }
    }

    /// Whether VS Code is installed is the machine's business, but the
    /// search must answer without panicking and must name a real file.
    #[test]
    fn locate_answers_with_an_executable_or_nothing() {
        match locate() {
            Some(path) => {
                println!("found {}", path.display());
                assert!(path.is_file(), "{} is not a file", path.display());
            }
            None => println!("no Visual Studio Code on this machine"),
        }
    }

    /// Ignored by default because it opens a real editor window.
    /// `cargo test opens_a_folder -- --ignored --nocapture`
    #[test]
    #[ignore = "opens a Visual Studio Code window"]
    fn opens_a_folder_for_real() {
        if locate().is_none() {
            println!("no Visual Studio Code on this machine; nothing to try");
            return;
        }
        let target = std::env::var("SHUTTLEFILES_VSCODE_TARGET").unwrap_or_else(|_| {
            let dir = std::env::temp_dir().join("shuttlefiles-vscode-test");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("hello.txt"), "hello").unwrap();
            dir.to_string_lossy().to_string()
        });
        println!("opening {}", target);
        open(&[target]).expect("launch");
    }
}
