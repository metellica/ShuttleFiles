//! Native shell integration.
//!
//! Home for the Windows-only features from the requirements that cannot
//! be expressed through the WebView:
//!
//! * **Clipboard interop (R5)** — implemented in [`clipboard`]:
//!   `CF_HDROP` + `Preferred DropEffect` so Copy/Cut/Paste works
//!   against Explorer.
//! * **Third-party context menus (R7)** — [`menu`] talks to the
//!   `shellmenu` helper process, which hosts `IContextMenu` and every
//!   registered extension (7-Zip, TortoiseGit, WinMerge, …). It runs
//!   out of process on purpose: the shell loads those extensions' DLLs
//!   into whoever asks for the menu, and a faulty one must not be able
//!   to take the browser down with it.

pub mod clipboard;
pub mod menu;
