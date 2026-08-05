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
//! * **Opening files** — [`launch`] runs the associated program with
//!   the item's own folder as the working directory, the way Explorer
//!   does, so scripts that reference their siblings by relative path
//!   still work.
//! * **Visual Studio Code** — [`vscode`] finds the installed editor and
//!   hands it a whole selection at once, which neither the association
//!   nor the text-editor setting can do.

pub mod clipboard;
pub mod launch;
pub mod menu;
pub mod terminal;
pub mod vscode;
