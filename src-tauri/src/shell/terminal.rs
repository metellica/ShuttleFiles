//! Detect available terminal environments: system CMD, PowerShell,
//! Visual Studio Developer Command Prompts/PowerShells, and Git Bash.
//!
//! Results are cached in a `OnceLock` because the set of installed
//! terminals cannot change while ShuttleFiles is running.

use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A launchable terminal environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEntry {
    /// Unique id, e.g. "cmd", "powershell", "vs2022-cmd", "git-bash".
    pub id: String,
    /// Human-readable label for menus.
    pub label: String,
    /// Grouping category.
    pub group: TerminalGroup,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalGroup {
    System,
    VisualStudio,
    Git,
}

/// Cached list of detected terminals.
static TERMINALS: OnceLock<Vec<TerminalEntry>> = OnceLock::new();

/// Returns the list of available terminals on this machine.
pub fn list() -> &'static [TerminalEntry] {
    TERMINALS.get_or_init(detect_all)
}

/// Opens a terminal of the given `id` in `cwd`.
pub fn open(id: &str, cwd: &Path) -> crate::error::AppResult<()> {
    let entries = list();
    if !entries.iter().any(|e| e.id == id) {
        return Err(crate::error::AppError::Config(format!(
            "Terminal '{}' is not available",
            id
        )));
    }
    imp::spawn(id, cwd)
}

fn detect_all() -> Vec<TerminalEntry> {
    let mut out = Vec::new();

    // --- System terminals ---
    out.push(TerminalEntry {
        id: "cmd".into(),
        label: "Command Prompt".into(),
        group: TerminalGroup::System,
    });
    out.push(TerminalEntry {
        id: "powershell".into(),
        label: "PowerShell".into(),
        group: TerminalGroup::System,
    });

    // --- Visual Studio Developer terminals ---
    #[cfg(windows)]
    {
        for vs in imp::detect_visual_studio() {
            out.push(TerminalEntry {
                id: format!("{}-cmd", vs.id),
                label: format!("Developer Command Prompt ({})", vs.display_name),
                group: TerminalGroup::VisualStudio,
            });
            out.push(TerminalEntry {
                id: format!("{}-ps", vs.id),
                label: format!("Developer PowerShell ({})", vs.display_name),
                group: TerminalGroup::VisualStudio,
            });
        }
    }

    // --- Git Bash ---
    if imp::find_git_bash().is_some() {
        out.push(TerminalEntry {
            id: "git-bash".into(),
            label: "Git Bash".into(),
            group: TerminalGroup::Git,
        });
    }

    out
}

#[cfg(windows)]
mod imp {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use crate::error::{AppError, AppResult};

    /// A detected Visual Studio installation.
    pub struct VsInstall {
        pub id: String,
        pub display_name: String,
        pub vcvars_path: PathBuf,
    }

    /// Detect Visual Studio installations via vswhere or known paths.
    pub fn detect_visual_studio() -> Vec<VsInstall> {
        let mut installs = Vec::new();
        let mut seen = HashSet::new();

        // Try vswhere first (ships with VS 2017+).
        if let Some(vswhere) = find_vswhere() {
            if let Ok(output) = std::process::Command::new(&vswhere)
                .args([
                    "-all",
                    "-products",
                    "*",
                    "-prerelease",
                    "-format",
                    "json",
                    "-utf8",
                ])
                .output()
            {
                if let Ok(entries) = serde_json::from_slice::<Vec<VsWhereEntry>>(&output.stdout) {
                    for entry in entries {
                        let vcvars = PathBuf::from(&entry.installation_path)
                            .join("Common7\\Tools\\VsDevCmd.bat");
                        if vcvars.is_file()
                            && seen.insert(vcvars.to_string_lossy().to_ascii_lowercase())
                        {
                            let version = entry
                                .installation_version
                                .split('.')
                                .next()
                                .unwrap_or("unknown");
                            let id = format!("vs{}-{}", version, slug(&entry.display_name));
                            installs.push(VsInstall {
                                id,
                                display_name: entry.display_name,
                                vcvars_path: vcvars,
                            });
                        }
                    }
                }
            }
        }

        // Merge well-known locations too: this covers portable/incomplete
        // registrations without dropping Build Tools beside a full VS install.
        let mut bases = Vec::new();
        if let Ok(pf) = std::env::var("ProgramFiles") {
            bases.push(PathBuf::from(pf).join("Microsoft Visual Studio"));
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            bases.push(PathBuf::from(pf86).join("Microsoft Visual Studio"));
        }
        let years = ["2022", "2019", "2017"];
        let editions = [
            "Enterprise",
            "Professional",
            "Community",
            "BuildTools",
            "Preview",
        ];

        for base in &bases {
            for year in years {
                for edition in editions {
                    let vcvars = base
                        .join(year)
                        .join(edition)
                        .join("Common7\\Tools\\VsDevCmd.bat");
                    if vcvars.is_file()
                        && seen.insert(vcvars.to_string_lossy().to_ascii_lowercase())
                    {
                        let id = format!("vs{}-{}", year, edition.to_ascii_lowercase());
                        let label = format!("VS {} {}", year, edition);
                        installs.push(VsInstall {
                            id,
                            display_name: label,
                            vcvars_path: vcvars,
                        });
                    }
                }
            }
        }

        installs
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct VsWhereEntry {
        installation_path: String,
        display_name: String,
        #[serde(default)]
        installation_version: String,
    }

    fn find_vswhere() -> Option<PathBuf> {
        // The canonical location.
        let pf86 = std::env::var("ProgramFiles(x86)").ok()?;
        let path = PathBuf::from(pf86).join("Microsoft Visual Studio\\Installer\\vswhere.exe");
        path.is_file().then_some(path)
    }

    fn slug(name: &str) -> String {
        name.chars()
            .filter_map(|c| {
                if c.is_ascii_alphanumeric() {
                    Some(c.to_ascii_lowercase())
                } else if c == ' ' || c == '-' {
                    Some('-')
                } else {
                    None
                }
            })
            .collect()
    }

    /// Find Git Bash (git-bash.exe).
    pub fn find_git_bash() -> Option<PathBuf> {
        // Registry: Git for Windows writes its install path here.
        if let Some(path) = git_bash_from_registry() {
            return Some(path);
        }
        // Common installation paths.
        for root in ["C:\\Program Files\\Git", "C:\\Program Files (x86)\\Git"] {
            let exe = PathBuf::from(root).join("git-bash.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
        // PATH fallback.
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let exe = dir.join("git-bash.exe");
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
        None
    }

    fn git_bash_from_registry() -> Option<PathBuf> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
        };

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        let sub_key = wide(r"Software\GitForWindows");
        let value_name = wide("InstallPath");

        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let mut bytes: u32 = 0;
            let rc = unsafe {
                RegGetValueW(
                    root,
                    PCWSTR(sub_key.as_ptr()),
                    PCWSTR(value_name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    None,
                    Some(&mut bytes),
                )
            };
            if rc != ERROR_SUCCESS || bytes < 4 {
                continue;
            }
            let mut buf = vec![0u16; bytes as usize / 2];
            let rc = unsafe {
                RegGetValueW(
                    root,
                    PCWSTR(sub_key.as_ptr()),
                    PCWSTR(value_name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut bytes),
                )
            };
            if rc != ERROR_SUCCESS {
                continue;
            }
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let install = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
            let exe = install.join("git-bash.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
        None
    }

    /// Spawn the terminal identified by `id` in `cwd`.
    pub fn spawn(id: &str, cwd: &Path) -> AppResult<()> {
        match id {
            "cmd" => std::process::Command::new("cmd.exe")
                .arg("/K")
                .arg(format!("cd /d \"{}\"", cwd.display()))
                .current_dir(cwd)
                .spawn()
                .map(|_| ())
                .map_err(|e| AppError::Io(format!("Cannot start cmd: {}", e))),
            "powershell" => {
                // Prefer pwsh (PowerShell 7+) over Windows PowerShell.
                let exe = if which_exists("pwsh.exe") {
                    "pwsh.exe"
                } else {
                    "powershell.exe"
                };
                std::process::Command::new(exe)
                    .arg("-NoExit")
                    .arg("-Command")
                    .arg(format!("Set-Location '{}'", cwd.display()))
                    .current_dir(cwd)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| AppError::Io(format!("Cannot start PowerShell: {}", e)))
            }
            "git-bash" => {
                let exe = find_git_bash()
                    .ok_or_else(|| AppError::Config("Git Bash is not installed".into()))?;
                std::process::Command::new(exe)
                    .arg(format!("--cd={}", cwd.display()))
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| AppError::Io(format!("Cannot start Git Bash: {}", e)))
            }
            other => {
                // Visual Studio developer terminals: "<vsid>-cmd" or "<vsid>-ps"
                let vs_installs = detect_visual_studio();
                if let Some(suffix) = other.strip_suffix("-cmd") {
                    if let Some(vs) = vs_installs.iter().find(|v| v.id == suffix) {
                        return spawn_vs_cmd(&vs.vcvars_path, cwd);
                    }
                }
                if let Some(suffix) = other.strip_suffix("-ps") {
                    if let Some(vs) = vs_installs.iter().find(|v| v.id == suffix) {
                        return spawn_vs_ps(&vs.vcvars_path, cwd);
                    }
                }
                Err(AppError::Config(format!("Unknown terminal: {}", other)))
            }
        }
    }

    fn spawn_vs_cmd(vcvars: &Path, cwd: &Path) -> AppResult<()> {
        // Start cmd, call VsDevCmd.bat, stay open.
        let script = format!(
            "cd /d \"{}\" && call \"{}\" && cmd /K",
            cwd.display(),
            vcvars.display()
        );
        std::process::Command::new("cmd.exe")
            .args(["/K", &script])
            .current_dir(cwd)
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::Io(format!("Cannot start VS Developer CMD: {}", e)))
    }

    fn spawn_vs_ps(vcvars: &Path, cwd: &Path) -> AppResult<()> {
        let exe = if which_exists("pwsh.exe") {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        let escaped_cwd = super::powershell_quote(&cwd.to_string_lossy());
        let cmd = format!(
            "{}; Set-Location '{}'",
            super::powershell_vs_environment_command(vcvars),
            escaped_cwd
        );
        std::process::Command::new(exe)
            .args(["-NoExit", "-Command", &cmd])
            .current_dir(cwd)
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::Io(format!("Cannot start VS Developer PS: {}", e)))
    }

    fn which_exists(name: &str) -> bool {
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
            .unwrap_or(false)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};

    use crate::error::{AppError, AppResult};

    pub fn find_git_bash() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("bash"))
            .find(|candidate| candidate.is_file())
    }

    pub fn spawn(id: &str, cwd: &Path) -> AppResult<()> {
        let (exe, args): (&str, Vec<&str>) = match id {
            "cmd" => ("bash", vec![]),
            "powershell" => ("pwsh", vec![]),
            "git-bash" => ("bash", vec![]),
            _ => return Err(AppError::Config(format!("Unknown terminal: {}", id))),
        };
        std::process::Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .spawn()
            .map(|_| ())
            .map_err(|e| AppError::Io(format!("Cannot start {}: {}", exe, e)))
    }
}

// --- Public accessors for the embedded terminal manager ---

/// Find the Git Bash executable path.
pub fn find_git_bash_path() -> Option<std::path::PathBuf> {
    imp::find_git_bash()
}

/// Find the VsDevCmd.bat path for a VS installation whose id matches `vs_id_prefix`.
/// The prefix is the part before `-cmd`/`-ps`, e.g. "vs17-visual-studio-professional-2022".
#[cfg(windows)]
pub fn find_vcvars_for(vs_id_prefix: &str) -> Option<std::path::PathBuf> {
    imp::detect_visual_studio()
        .into_iter()
        .find(|vs| vs.id == vs_id_prefix)
        .map(|vs| vs.vcvars_path)
}

#[cfg(not(windows))]
pub fn find_vcvars_for(_vs_id_prefix: &str) -> Option<std::path::PathBuf> {
    None
}

fn powershell_quote(value: &str) -> String {
    value.replace('\'', "''")
}

/// PowerShell statement that imports the environment produced by VsDevCmd.bat
/// into the current process without executing Launch-VsDevShell.ps1, which exits.
pub fn powershell_vs_environment_command(vcvars: &Path) -> String {
    let vcvars = powershell_quote(&vcvars.to_string_lossy());
    format!(
        "$envBlock = & cmd.exe /d /s /c '\"{}\" >nul 2>&1 && set'; \
         foreach ($line in $envBlock) {{ if ($line -match '^([^=]+)=(.*)$') {{ \
         [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }} }}",
        vcvars
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_runs_without_panic() {
        let terminals = detect_all();
        // At minimum, system terminals are always present.
        assert!(terminals.len() >= 2);
        assert!(terminals.iter().any(|t| t.id == "cmd"));
        assert!(terminals.iter().any(|t| t.id == "powershell"));
        println!("Detected terminals:");
        for t in &terminals {
            println!("  {} - {}", t.id, t.label);
        }
    }

    #[test]
    fn powershell_vs_environment_does_not_run_the_exiting_launcher() {
        let command =
            powershell_vs_environment_command(Path::new(r"C:\VS\Common7\Tools\VsDevCmd.bat"));
        assert!(command.contains("VsDevCmd.bat"));
        assert!(!command.contains("Launch-VsDevShell.ps1"));
    }
}
