//! Helper process that hosts the Windows shell context menu.
//!
//! Third-party context menu entries (7-Zip, TortoiseGit, WinMerge, …)
//! are classic in-process COM servers implementing `IContextMenu`. The
//! shell loads **their DLLs into whatever process asks for the menu**,
//! then runs their code for painting and for the invoked command. That
//! is why this lives in its own executable: a faulty or slow extension
//! can hang or crash this helper without touching the file browser, its
//! tabs, or an in-flight copy.
//!
//! It is deliberately not a long-running service — one menu, one
//! process, then exit, so no extension DLL ever stays resident.
//!
//! Protocol: a JSON request on stdin, a JSON response on stdout.
//!
//! ```text
//! {"mode":"list","paths":["C:\\a.txt"]}
//! {"mode":"show","paths":["C:\\a.txt"],"x":100,"y":200}
//! ```
//!
//! `list` reports what the shell would show without displaying anything,
//! which makes the whole COM path testable from a script. `show`
//! displays the real menu and invokes the chosen command.

// Console in debug builds so the protocol can be driven by hand;
// windowless in release so no console flashes when the menu opens.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    /// `list` | `show`
    mode: String,
    paths: Vec<String>,
    /// Screen coordinates for `show`.
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct MenuItemInfo {
    /// Command id, relative to the menu's first id. `null` for
    /// separators and for entries that only open a submenu.
    id: Option<u32>,
    label: String,
    /// The shell's language-independent verb, when it exposes one.
    verb: String,
    separator: bool,
    enabled: bool,
    default: bool,
    /// Whether the shell attached a submenu. `children` can still be
    /// empty: several extensions build their submenu lazily, on the
    /// `WM_INITMENUPOPUP` that only arrives once the menu is shown.
    has_submenu: bool,
    children: Vec<MenuItemInfo>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    ok: bool,
    /// Populated by `list`.
    items: Vec<MenuItemInfo>,
    /// Verb invoked by `show`; empty when the menu was dismissed.
    invoked: String,
    error: String,
}

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        emit(Response {
            ok: false,
            error: "Cannot read request".into(),
            ..Default::default()
        });
        return;
    }

    let response = match serde_json::from_str::<Request>(&raw) {
        Ok(request) => run(request),
        Err(e) => Response {
            ok: false,
            error: format!("Bad request: {}", e),
            ..Default::default()
        },
    };
    emit(response);
}

fn emit(response: Response) {
    let text = serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"ok":false,"items":[],"invoked":"","error":"Cannot serialize response"}"#.into()
    });
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

#[cfg(windows)]
fn run(request: Request) -> Response {
    match imp::run(&request) {
        Ok(response) => response,
        Err(e) => Response {
            ok: false,
            error: e,
            ..Default::default()
        },
    }
}

#[cfg(not(windows))]
fn run(_request: Request) -> Response {
    Response {
        ok: false,
        error: "Shell context menus are only available on Windows".into(),
        ..Default::default()
    }
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use windows::core::{Interface, BOOL, PCSTR, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        IContextMenu, IContextMenu2, IContextMenu3, IShellFolder, SHBindToParent,
        SHParseDisplayName, CMINVOKECOMMANDINFOEX,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        GetMenuItemCount, GetMenuItemInfoW, RegisterClassW, SetForegroundWindow,
        TrackPopupMenuEx, HMENU, MENUITEMINFOW, MFS_DEFAULT, MFS_DISABLED, MFS_GRAYED,
        MFT_SEPARATOR, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_DRAWITEM, WM_INITMENUPOPUP,
        WM_MEASUREITEM, WM_MENUCHAR, WNDCLASSW, WS_OVERLAPPED,
    };

    use super::{MenuItemInfo, Request, Response};

    /// Command ids handed to the shell. Anything it returns is an offset
    /// from `ID_FIRST`, which is what `InvokeCommand` expects back.
    const ID_FIRST: u32 = 1;
    const ID_LAST: u32 = 0x7FFF;

    /// `CMF_NORMAL | CMF_ITEMMENU` — a menu for a selection, which is
    /// what the shell shows when you right-click files in Explorer.
    const CMF_ITEMMENU: u32 = 0x0000_0080;

    /// `GetCommandString` type for the ANSI verb.
    const GCS_VERBA: u32 = 0x0000_0000;

    /// `CMIC_MASK_UNICODE` — tells the shell the `*W` members of
    /// `CMINVOKECOMMANDINFOEX` are filled in. Not re-exported by the
    /// `windows` crate, so it is spelled out here.
    const CMIC_MASK_UNICODE: u32 = 0x0000_4000;

    thread_local! {
        /// The live menu's `IContextMenu2/3`, so the window procedure
        /// can hand it the messages it needs. 7-Zip's submenu is
        /// owner-drawn: without this forwarding it renders blank.
        static ACTIVE_MENU: RefCell<Option<IContextMenu>> = const { RefCell::new(None) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// COM apartment guard: shell extensions require an STA, and
    /// `CoUninitialize` must run even on the error paths.
    struct Apartment;

    impl Apartment {
        fn enter() -> Result<Self, String> {
            let hr = unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            };
            if hr.is_err() {
                return Err(format!("CoInitializeEx failed: {:?}", hr));
            }
            Ok(Apartment)
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// Owns a PIDL allocated by the shell.
    struct Pidl(*mut ITEMIDLIST);

    impl Drop for Pidl {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(self.0 as *const _)) };
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // An extension panicking must not unwind into the message pump.
        let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
                return None;
            }
            ACTIVE_MENU.with(|slot| {
                let borrow = slot.borrow();
                let menu = borrow.as_ref()?;
                // IContextMenu3 also handles WM_MENUCHAR (accelerators
                // inside owner-drawn submenus); fall back to 2.
                if let Ok(cm3) = menu.cast::<IContextMenu3>() {
                    let mut result = LRESULT(0);
                    if unsafe { cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }.is_ok()
                    {
                        return Some(result);
                    }
                }
                if msg == WM_MENUCHAR {
                    return None;
                }
                let cm2 = menu.cast::<IContextMenu2>().ok()?;
                unsafe { cm2.HandleMenuMsg(msg, wparam, lparam) }.ok()?;
                Some(LRESULT(0))
            })
        }))
        .unwrap_or(None);

        match handled {
            Some(result) => result,
            None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    /// Hidden owner window for the popup. `TrackPopupMenuEx` needs a
    /// real window to send its owner-draw messages to.
    fn create_owner_window() -> Result<HWND, String> {
        let class_name = wide("ShuttleFilesShellMenu");
        let instance = unsafe { GetModuleHandleW(None) }
            .map_err(|e| format!("GetModuleHandleW failed: {}", e))?;

        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // A duplicate class registration is fine; the class survives.
        unsafe { RegisterClassW(&class) };

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW failed: {}", e))?;
        Ok(hwnd)
    }

    /// Resolve the shell's context menu for `paths`.
    ///
    /// The shell can only build one menu for items in a single folder,
    /// which matches how a selection works in the UI anyway.
    fn context_menu_for(paths: &[String], hwnd: HWND) -> Result<IContextMenu, String> {
        let first = paths.first().ok_or("No paths given")?;
        let parent_dir = Path::new(first)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(first));

        let mut pidls: Vec<Pidl> = Vec::with_capacity(paths.len());
        let mut folder: Option<IShellFolder> = None;

        for path in paths {
            if Path::new(path).parent().map(Path::to_path_buf).as_ref() != Some(&parent_dir) {
                return Err("All items must be in the same folder".into());
            }

            let wide_path = wide(path);
            let mut full: *mut ITEMIDLIST = std::ptr::null_mut();
            unsafe { SHParseDisplayName(PCWSTR(wide_path.as_ptr()), None, &mut full, 0, None) }
                .map_err(|e| format!("Cannot resolve {}: {}", path, e))?;
            let full = Pidl(full);

            // SHBindToParent hands back a pointer *into* the full PIDL,
            // so `full` has to outlive the child - hence keeping both.
            let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
            let parent: IShellFolder = unsafe { SHBindToParent(full.0, Some(&mut child)) }
                .map_err(|e| format!("Cannot bind parent of {}: {}", path, e))?;

            if folder.is_none() {
                folder = Some(parent);
            }
            pidls.push(full);
            // Child pointers are gathered after the loop, from the
            // owned full PIDLs, to keep lifetimes obvious.
            let _ = child;
        }

        let folder = folder.ok_or("No shell folder resolved")?;

        // Re-derive the child PIDLs now that every parent is owned.
        let mut children: Vec<*const ITEMIDLIST> = Vec::with_capacity(pidls.len());
        for pidl in &pidls {
            let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
            let _parent: IShellFolder = unsafe { SHBindToParent(pidl.0, Some(&mut child)) }
                .map_err(|e| format!("Cannot bind parent: {}", e))?;
            children.push(child as *const _);
        }

        let menu: IContextMenu = unsafe { folder.GetUIObjectOf(hwnd, &children, None) }
            .map_err(|e| format!("Cannot get context menu: {}", e))?;
        Ok(menu)
    }

    /// Read the shell's language-independent verb for a command, which
    /// is far more useful for logging than a localised label.
    fn verb_for(menu: &IContextMenu, id: u32) -> String {
        let mut buf = [0u8; 128];
        let ok = unsafe {
            menu.GetCommandString(
                (id - ID_FIRST) as usize,
                GCS_VERBA,
                None,
                windows::core::PSTR(buf.as_mut_ptr()),
                buf.len() as u32,
            )
        }
        .is_ok();
        if !ok {
            return String::new();
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }

    fn read_menu(menu: &IContextMenu, hmenu: HMENU, depth: u32) -> Vec<MenuItemInfo> {
        // Guard against a pathological (or hostile) extension nesting
        // submenus without end.
        if depth > 6 {
            return Vec::new();
        }
        let count = unsafe { GetMenuItemCount(Some(hmenu)) };
        let mut items = Vec::new();

        for index in 0..count.max(0) {
            let mut label = [0u16; 260];
            let mut info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_STRING | MIIM_SUBMENU,
                dwTypeData: PWSTR(label.as_mut_ptr()),
                cch: label.len() as u32 - 1,
                ..Default::default()
            };
            if unsafe { GetMenuItemInfoW(hmenu, index as u32, true, &mut info) }.is_err() {
                continue;
            }

            if info.fType.0 & MFT_SEPARATOR.0 != 0 {
                items.push(MenuItemInfo {
                    separator: true,
                    enabled: false,
                    ..Default::default()
                });
                continue;
            }

            let text = String::from_utf16_lossy(&label[..info.cch as usize]);
            // Owner-drawn entries carry no string; the extension paints
            // them itself. They are still invocable, so keep them with a
            // placeholder rather than dropping them.
            let label = if text.trim().is_empty() {
                "(custom item)".to_string()
            } else {
                text.replace('&', "")
            };

            let has_submenu = !info.hSubMenu.is_invalid();
            let children = if has_submenu {
                read_menu(menu, info.hSubMenu, depth + 1)
            } else {
                Vec::new()
            };

            let id = (!has_submenu && info.wID >= ID_FIRST && info.wID <= ID_LAST)
                .then_some(info.wID - ID_FIRST);

            items.push(MenuItemInfo {
                verb: id.map(|_| verb_for(menu, info.wID)).unwrap_or_default(),
                id,
                label,
                separator: false,
                enabled: info.fState.0 & (MFS_DISABLED.0 | MFS_GRAYED.0) == 0,
                default: info.fState.0 & MFS_DEFAULT.0 != 0,
                has_submenu,
                children,
            });
        }
        items
    }

    fn invoke(menu: &IContextMenu, hwnd: HWND, id: u32, directory: &str) -> Result<(), String> {
        let dir = format!("{}\0", directory);
        let mut info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_UNICODE,
            hwnd,
            // The low word of lpVerb doubles as a command offset when
            // the pointer has no high word - the shell's convention for
            // "invoke by id".
            lpVerb: PCSTR(id as usize as *const u8),
            lpVerbW: PCWSTR(id as usize as *const u16),
            lpDirectory: PCSTR(dir.as_ptr()),
            nShow: 1, // SW_SHOWNORMAL
            ..Default::default()
        };
        unsafe { menu.InvokeCommand(&mut info as *mut _ as *const _) }
            .map_err(|e| format!("Command failed: {}", e))
    }

    pub fn run(request: &Request) -> Result<Response, String> {
        if request.paths.is_empty() {
            return Err("No paths given".into());
        }
        let _apartment = Apartment::enter()?;
        let hwnd = create_owner_window()?;

        let result = (|| -> Result<Response, String> {
            let menu = context_menu_for(&request.paths, hwnd)?;
            let hmenu = unsafe { CreatePopupMenu() }
                .map_err(|e| format!("CreatePopupMenu failed: {}", e))?;

            let query = unsafe {
                menu.QueryContextMenu(hmenu, 0, ID_FIRST, ID_LAST, CMF_ITEMMENU)
            };
            if query.is_err() {
                unsafe { DestroyMenu(hmenu) }.ok();
                return Err(format!("QueryContextMenu failed: {:?}", query));
            }

            let response = match request.mode.as_str() {
                "list" => Response {
                    ok: true,
                    items: read_menu(&menu, hmenu, 0),
                    ..Default::default()
                },
                "show" => {
                    // Owner-draw messages arrive while the menu is up.
                    ACTIVE_MENU.with(|slot| *slot.borrow_mut() = Some(menu.clone()));
                    // Without foreground ownership the menu would not
                    // close when the user clicks elsewhere.
                    let _ = unsafe { SetForegroundWindow(hwnd) };

                    let chosen = unsafe {
                        TrackPopupMenuEx(
                            hmenu,
                            (TPM_RETURNCMD | TPM_RIGHTBUTTON).0,
                            request.x,
                            request.y,
                            hwnd,
                            None,
                        )
                    };
                    ACTIVE_MENU.with(|slot| *slot.borrow_mut() = None);

                    let mut response = Response {
                        ok: true,
                        ..Default::default()
                    };
                    if chosen.0 != 0 {
                        let id = chosen.0 as u32;
                        response.invoked = verb_for(&menu, id);
                        let dir = Path::new(&request.paths[0])
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        invoke(&menu, hwnd, id - ID_FIRST, &dir)?;
                    }
                    response
                }
                other => return Err(format!("Unknown mode: {}", other)),
            };

            unsafe { DestroyMenu(hmenu) }.ok();
            Ok(response)
        })();

        unsafe { DestroyWindow(hwnd) }.ok();
        result
    }

    /// Silences an unused-import warning when the BOOL/POINT types are
    /// only referenced through generated signatures.
    #[allow(dead_code)]
    fn _types(_: BOOL, _: POINT) {}
}
