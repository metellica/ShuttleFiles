# ShuttleFiles

A lightweight local file browser built with **Tauri 2 + Vue 3 + Rust**, sharing
the tech stack and visual language of [ShuttleSFTP](../ShuttleSFTP).

## Status

Navigation, tabs, fast dial, favorites, system-clipboard interop, background
file operations tuned for large trees, and native third-party context menus all
work. Remaining: remote shares (R4).

## Implemented

- 🗂 **Multi-tab** — per-tab Back/Forward history, drag to reorder, middle-click
  to close, duplicate/close-others context menu, tabs restored on restart
- 🎯 **Fast Dial** — a new tab opens a dial page with drives (capacity bars),
  quick-access folders, favorites (drag to reorder) and most-visited folders
- ⭐ **Favorites** — star the current folder from the toolbar or any folder from
  its context menu; stored as plain JSON
- 🧭 **Address bar** — clickable breadcrumbs, click-to-edit, Copy path,
  Paste & Go, `%ENV%` / `~` expansion, `/` → `\` normalisation, UNC input
- 📋 **Details view** — sortable Name / Size / Type / Modified, Ctrl+click and
  Shift+click multi-select, hidden files dimmed, live filter box
- 🔍 **Adjustable row size** — Small / Medium / Large presets plus stepless
  zoom from 75 % to 250 % via Ctrl+wheel, `Ctrl+=` / `Ctrl+-` or the slider;
  row height, text, icons and column widths all scale together and the
  setting is remembered
- 📎 **System clipboard interop** — Ctrl+C / Ctrl+X / Ctrl+V use the real
  Windows clipboard (`CF_HDROP` + `Preferred DropEffect`), so files copied here
  paste into Explorer and files copied in Explorer paste here, Cut included
- ⏳ **Background file operations** — copy / move / delete run as cancellable
  jobs with a live progress bar, per-file detail, throughput and ETA in a
  status panel; browsing, opening tabs and starting more jobs stay responsive
- 🚀 **Tuned for large trees** — parallel scan, a bounded worker pool and
  kernel-side `CopyFile2`; see [Performance](#performance) for measurements
- ✂️ **File operations** — Rename, Delete (with confirmation), New Folder,
  collision-safe `name (2)` destinations, same-volume moves via rename
- 🖱 **System integration** — open files with their default app, Show in Explorer
- 📝 **Editor for text files** — pick one program in ⚙ Settings and every text
  extension opens with it, on double click and from the context menu, without
  touching Windows' per-extension associations; the extension list is editable
  and "Open with System Default" stays one click away
- 🧩 **Third-party context menus** — "More options" opens the real Windows shell
  menu, so 7-Zip, WinMerge, TortoiseGit, PowerToys and everything else you have
  installed work exactly as they do in Explorer; hosted out of process

## Not implemented yet

| Requirement | Plan |
|---|---|
| Remote shares (R4) | `WNetAddConnection2W` for credentials, `NetShareEnum` to list shares. UNC paths already parse and browse today |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri 2 |
| Frontend | Vue 3 + TypeScript + Pinia |
| Backend | Rust (async, tokio) |
| Bundler | Vite |

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) ≥ 22

### Development

```bash
npm install
npx tauri dev
```

### Build

```bash
npx tauri build
```

### Checks

```bash
npm run type-check                 # vue-tsc
npm run build-only                 # vite build
cd src-tauri && cargo test         # path model, clipboard, ops engine, IPC, shell menu

# Benchmarks (write hundreds of MB, excluded from the normal run)
cd src-tauri && cargo test --release bench_ -- --ignored --nocapture

# Inspect the shell menu for a path without opening it
echo '{"mode":"list","paths":["C:\\Windows\\win.ini"]}' | ./src-tauri/target/debug/shellmenu.exe
```

The Rust suite includes integration tests that drive the real commands through
Tauri's mock runtime, and others that resolve genuine shell menus against
whatever extensions are installed — both without a GUI.

## Project Structure

```
ShuttleFiles/
├── src/                       # Vue 3 frontend
│   ├── components/
│   │   ├── browser/           # FileBrowser, FileList, FastDial, PathBar
│   │   ├── common/            # ContextMenu, PromptDialog
│   │   └── layout/            # TabBar, Toolbar, OperationBar, DensityControl
│   ├── composables/           # Tauri IPC wrappers, formatting, prompt
│   ├── stores/                # Pinia state (tabs, clipboard, places, operations, view)
│   └── types/                 # TypeScript interfaces
├── src-tauri/                 # Rust backend
│   ├── app-icon.svg           # Icon source; regenerate with `npx tauri icon`
│   ├── src/
│   │   ├── bin/shellmenu.rs   # Out-of-process host for shell context menus
│   │   ├── fs/                # path model, listing, drives
│   │   ├── ops/               # background job registry + copy/move/delete engine
│   │   ├── config/            # JSON store, favorites & history
│   │   ├── shell/             # native Win32 integration (clipboard, menu bridge)
│   │   └── commands/          # Tauri IPC command handlers
│   └── tests/                 # Integration tests (Tauri mock runtime, shell menu)
├── scripts/stage-helper.mjs   # Stages shellmenu.exe for bundling
└── vite.config.ts
```

### Background operations

Copy / move / delete never block the UI. `start_operation` registers a job and
returns its id immediately, then a dedicated OS thread scans for totals and does
the work, pushing throttled `fileop:update` events (at most one per 120 ms).

Two deliberate choices:

- **A dedicated `std::thread`, not `tokio::spawn_blocking`.** `start_operation`
  is a synchronous Tauri command with no reactor installed, and a copy can hold
  its thread for minutes — which would starve the blocking pool that every
  directory listing shares.
- **`Progress` is a trait.** The engine is exercised in unit tests with a
  counting stub, no Tauri app handle required.

## Performance

Large-tree throughput (requirement R6) comes from three things:

1. **Parallel scan.** `jwalk` walks the tree, and the per-file `stat` happens
   inside `process_read_dir` — on the walker's thread pool instead of serially
   on the consuming thread. Only *directories* are materialised, so a tree with
   millions of files still has bounded memory.
2. **A bounded worker pool.** One thread enumerates and feeds a backpressured
   channel; 2–8 workers do the IO. This is a large win wherever each operation
   is latency-bound (SSD/NVMe, network shares).
3. **Kernel-side copy.** On Windows every file goes through `CopyFile2`, which
   copies in the kernel, preserves timestamps and attributes, reports byte-level
   progress through its callback and can be cancelled mid-file.

Measured on this machine (`cargo test --release bench_ -- --ignored --nocapture`),
against the straightforward `std::fs` recursion, median of three runs:

| Workload | Baseline | This engine | Speed-up |
|---|---|---|---|
| Scan 10 000 files / 200 folders | 0.67 s | 0.10 s | **6.5×** |
| Copy the same tree | 6.6 s | 1.77 s | **3.8×** |
| Delete the same tree | 1.44 s | 0.60 s | **2.4×** |
| Copy one 512 MiB file | 3785 MiB/s | 3485 MiB/s | ~par, **plus progress + cancel** |

Two findings worth recording:

- `COPY_FILE_NO_BUFFERING` is documented as "recommended for very large file
  transfers", but measured **24 MiB/s against 3.3 GiB/s** on a 512 MiB file. It
  is not used; `CopyFile2` already switches strategy on its own.
- Delete is metadata-bound. The pool still helps, but the bigger reason to keep
  it is that `remove_dir_all` can report neither progress nor cancellation.

### Clipboard

Windows describes a file selection with `CF_HDROP` (a `DROPFILES` header plus a
double-NUL terminated wide path list) and the registered `Preferred DropEffect`
format, which is the only thing distinguishing a Copy from a Cut. The WebView
clipboard API cannot see either, so `shell/clipboard.rs` talks to Win32 directly.
Non-Windows builds fall back to an in-process clipboard.

### Third-party context menus

"More options" shows the genuine Windows shell menu, so anything registered on
the machine appears — verified against 7-Zip (including its nested *CRC SHA*
submenu), WinMerge, KDiff3 and PowerToys.

No extension is enumerated by hand. `IShellFolder::GetUIObjectOf(IID_IContextMenu)`
already returns the shell's *aggregated* menu, which is exactly what Explorer
asks for:

```
SHParseDisplayName → SHBindToParent → GetUIObjectOf → QueryContextMenu
                                                    → TrackPopupMenuEx
                                                    → InvokeCommand
```

Three things make this reliable:

- **It runs out of process.** The shell loads extension DLLs into whoever asks
  for the menu, and runs their code for painting and for the invoked command.
  `shellmenu.exe` is spawned per menu and exits with it, so a slow or crashing
  extension costs a throw-away process and nothing else — no tab, no in-flight
  copy. Nothing stays resident.
- **`IContextMenu2/3` messages are forwarded.** `WM_INITMENUPOPUP`,
  `WM_DRAWITEM`, `WM_MEASUREITEM` and `WM_MENUCHAR` go back to the extension;
  without this, owner-drawn entries such as 7-Zip's submenu render blank.
- **A `list` mode makes it testable.** It resolves the whole COM path and
  reports what the shell *would* show, without displaying anything, so the
  integration tests cover it headlessly.

Note that Windows 11's newer `IExplorerCommand` entries (MSIX sparse packages)
are only surfaced by Explorer itself. Classic handlers — which is what 7-Zip,
TortoiseGit, WinMerge and the like register — are unaffected.

### Icon

`src-tauri/app-icon.svg` is the single source; every platform size is generated
from it with `npx tauri icon src-tauri/app-icon.svg`.

It keeps the Shuttle family's shape language — squircle tile, one white glyph —
but uses a sky-to-blue tile and a folder rather than ShuttleSFTP's indigo tile
and free-standing shuttle, so the two are tellable apart in a taskbar: one is
mostly blue with a white mark, the other mostly white with a blue rim. The
shuttle survives as a knockout inside the folder. Proportions are driven by the
16–32 px case, where the folder silhouette has to carry the meaning on its own.

### Path model

Unlike ShuttleSFTP — which normalises everything to POSIX-style browse paths
(`/C:/Users/...`) so SFTP and local share one string format — ShuttleFiles keeps
**native** paths (`C:\Users\...`). Every Win32 shell API consumes native paths,
so staying native avoids a lossy conversion on every call.

The one virtual location is the root sentinel `""` ("This PC"), which lists
drives and renders as the fast dial.

## Keyboard Shortcuts

| Keys | Action |
|------|--------|
| `Ctrl+T` / `Ctrl+W` | New tab / close tab |
| `Alt+←` / `Alt+→` / `Alt+↑` | Back / Forward / Up |
| `Backspace` | Up one folder |
| `Ctrl+L` | Edit address bar |
| `F5` | Refresh |
| `Ctrl+A` | Select all |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / Cut / Paste (system clipboard — works with Explorer) |
| `F2` | Rename |
| `Delete` | Delete |
| `Ctrl+Shift+N` | New folder |
| `Ctrl+wheel` / `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Row size: zoom in / out / reset |

## Configuration

Plain JSON under `~/.config/shuttle-files/`, matching the ShuttleSFTP convention:

| File | Contents |
|------|----------|
| `favorites.json` | Starred folders shown on the fast dial |
| `recent.json` | Visit history (capped at 200) driving "Frequent" |
| `tabs.json` | Open tabs, restored on the next start |
| `view.json` | Row size |
| `open-with.json` | Program used for text files, and which extensions count as text |

## License

MIT
