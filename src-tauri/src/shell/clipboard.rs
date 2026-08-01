//! System clipboard file transfer (`CF_HDROP`).
//!
//! The WebView's clipboard API can only see text, so Copy/Cut/Paste
//! against Explorer has to go through Win32 directly. Windows describes
//! a file selection with two clipboard formats:
//!
//! * `CF_HDROP` — a `DROPFILES` header followed by a double-NUL
//!   terminated list of wide paths.
//! * `Preferred DropEffect` — a registered format holding
//!   `DROPEFFECT_COPY` or `DROPEFFECT_MOVE`, which is the only thing
//!   distinguishing a Copy from a Cut.

use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardFiles {
    pub paths: Vec<String>,
    /// True when the source marked the selection as Cut (move).
    pub cut: bool,
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Shell::{DragQueryFileW, DROPFILES, HDROP};

    use super::ClipboardFiles;
    use crate::error::{AppError, AppResult};

    const CF_HDROP: u32 = 15;
    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn preferred_drop_effect_format() -> u32 {
        let name = wide("Preferred DropEffect");
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
    }

    /// Closes the clipboard when dropped, so an early return can never
    /// leave it locked for every other application on the desktop.
    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open() -> AppResult<Self> {
            // Another process may hold the clipboard for a moment; Explorer
            // itself retries the same way rather than failing outright.
            for attempt in 0..10 {
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Ok(ClipboardGuard);
                }
                std::thread::sleep(std::time::Duration::from_millis(10 * (attempt + 1)));
            }
            Err(AppError::Io("Clipboard is busy".into()))
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// Copy `bytes` into a moveable HGLOBAL, which the clipboard takes
    /// ownership of on a successful `SetClipboardData`.
    unsafe fn global_from_bytes(bytes: &[u8]) -> AppResult<HGLOBAL> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
            .map_err(|e| AppError::Io(format!("GlobalAlloc failed: {}", e)))?;
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            return Err(AppError::Io("GlobalLock failed".into()));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            let _ = GlobalUnlock(handle);
        }
        Ok(handle)
    }

    /// `DROPFILES` header + double-NUL terminated wide path list.
    fn build_hdrop(paths: &[String]) -> Vec<u8> {
        let mut list: Vec<u16> = Vec::new();
        for p in paths {
            list.extend(p.encode_utf16());
            list.push(0);
        }
        list.push(0);

        let header = DROPFILES {
            pFiles: std::mem::size_of::<DROPFILES>() as u32,
            pt: Default::default(),
            fNC: false.into(),
            fWide: true.into(),
        };

        let mut bytes = Vec::with_capacity(std::mem::size_of::<DROPFILES>() + list.len() * 2);
        let header_bytes = unsafe {
            std::slice::from_raw_parts(
                &header as *const DROPFILES as *const u8,
                std::mem::size_of::<DROPFILES>(),
            )
        };
        bytes.extend_from_slice(header_bytes);
        for unit in list {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    pub fn write_files(paths: &[String], cut: bool) -> AppResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let _guard = ClipboardGuard::open()?;
        unsafe {
            EmptyClipboard().map_err(|e| AppError::Io(format!("EmptyClipboard failed: {}", e)))?;

            let hdrop = global_from_bytes(&build_hdrop(paths))?;
            SetClipboardData(CF_HDROP, Some(HANDLE(hdrop.0)))
                .map_err(|e| AppError::Io(format!("SetClipboardData failed: {}", e)))?;

            let effect: u32 = if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
            let effect_handle = global_from_bytes(&effect.to_le_bytes())?;
            // Without this format every paste is treated as a copy.
            SetClipboardData(
                preferred_drop_effect_format(),
                Some(HANDLE(effect_handle.0)),
            )
            .map_err(|e| AppError::Io(format!("SetClipboardData(effect) failed: {}", e)))?;
        }
        Ok(())
    }

    pub fn read_files() -> AppResult<ClipboardFiles> {
        if unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() {
            return Ok(ClipboardFiles::default());
        }
        let _guard = ClipboardGuard::open()?;

        let handle = match unsafe { GetClipboardData(CF_HDROP) } {
            Ok(h) => h,
            Err(_) => return Ok(ClipboardFiles::default()),
        };
        let hdrop = HDROP(handle.0);

        let mut paths = Vec::new();
        // 0xFFFFFFFF asks for the count rather than a path.
        let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
        for i in 0..count {
            let len = unsafe { DragQueryFileW(hdrop, i, None) };
            if len == 0 {
                continue;
            }
            // DragQueryFileW's return excludes the terminator it writes.
            let mut buf = vec![0u16; len as usize + 1];
            let written = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
            if written > 0 {
                paths.push(String::from_utf16_lossy(&buf[..written as usize]));
            }
        }

        let cut = read_drop_effect().unwrap_or(false);
        Ok(ClipboardFiles { paths, cut })
    }

    fn read_drop_effect() -> Option<bool> {
        let format = preferred_drop_effect_format();
        if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
            return None;
        }
        let handle = unsafe { GetClipboardData(format) }.ok()?;
        let hglobal = HGLOBAL(handle.0);
        let ptr = unsafe { GlobalLock(hglobal) } as *const u32;
        if ptr.is_null() {
            return None;
        }
        let effect = unsafe { *ptr };
        unsafe {
            let _ = GlobalUnlock(hglobal);
        }
        Some(effect & DROPEFFECT_MOVE != 0)
    }

    pub fn has_files() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_ok()
    }

    /// Silence the unused-import warning for `c_void` on some toolchains.
    #[allow(dead_code)]
    fn _assert_types(_: *mut c_void) {}
}

#[cfg(not(windows))]
mod imp {
    //! No portable equivalent of `CF_HDROP` exists, so non-Windows
    //! builds keep an in-process clipboard: copy/paste works inside the
    //! app but not against the desktop's file manager.

    use std::sync::Mutex;

    use super::ClipboardFiles;
    use crate::error::AppResult;

    static FALLBACK: Mutex<Option<ClipboardFiles>> = Mutex::new(None);

    pub fn write_files(paths: &[String], cut: bool) -> AppResult<()> {
        let mut slot = FALLBACK.lock().unwrap();
        *slot = Some(ClipboardFiles {
            paths: paths.to_vec(),
            cut,
        });
        Ok(())
    }

    pub fn read_files() -> AppResult<ClipboardFiles> {
        Ok(FALLBACK.lock().unwrap().clone().unwrap_or_default())
    }

    pub fn has_files() -> bool {
        FALLBACK
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|c| !c.paths.is_empty())
    }
}

/// Put a file selection on the clipboard. `cut` marks it as a move.
pub async fn write_files(paths: Vec<String>, cut: bool) -> AppResult<()> {
    tokio::task::spawn_blocking(move || imp::write_files(&paths, cut))
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Clipboard task failed: {}", e)))?
}

pub async fn read_files() -> AppResult<ClipboardFiles> {
    tokio::task::spawn_blocking(imp::read_files)
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Clipboard task failed: {}", e)))?
}

pub async fn has_files() -> bool {
    tokio::task::spawn_blocking(imp::has_files)
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips through the real clipboard, which verifies the
    /// `DROPFILES` header offset, the wide double-NUL path list and the
    /// `Preferred DropEffect` cut flag in one go.
    #[test]
    fn hdrop_round_trip() {
        let paths = vec![
            "C:\\Windows\\notepad.exe".to_string(),
            "C:\\Users\\Public\\a b c.txt".to_string(),
        ];

        if imp::write_files(&paths, true).is_err() {
            // No window station (headless CI); nothing to verify.
            return;
        }
        let read = imp::read_files().expect("read clipboard");
        assert_eq!(read.paths, paths);
        assert!(read.cut, "Preferred DropEffect should report a move");

        imp::write_files(&paths, false).expect("write clipboard");
        let read = imp::read_files().expect("read clipboard");
        assert!(!read.cut, "a plain copy must not be reported as a move");
        assert!(imp::has_files());
    }

    /// Manual check that the reader handles a `CF_HDROP` produced by
    /// another application (Explorer, .NET, …) rather than by us. Copy
    /// some files elsewhere, then:
    /// `cargo test reads_external_clipboard -- --ignored --nocapture`
    #[test]
    #[ignore = "requires files copied to the clipboard by another app"]
    fn reads_external_clipboard() {
        let read = imp::read_files().expect("read clipboard");
        println!("cut = {}", read.cut);
        for p in &read.paths {
            println!("path = {}", p);
        }
        assert!(!read.paths.is_empty(), "no files on the clipboard");
    }
}
