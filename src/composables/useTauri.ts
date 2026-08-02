import { invoke } from '@tauri-apps/api/core'
import type {
  DirListing,
  DriveInfo,
  Favorite,
  HashAlgo,
  OpenWithSettings,
  PlaceEntry,
  RecentEntry,
  SearchResult,
  TabSnapshot,
  ViewSettings,
} from '@/types/filesystem'
import type {
  ClipboardFiles,
  JobKind,
  JobOptions,
  JobState,
  ShellMenuResult,
} from '@/types/operations'

export const listDir = (path: string) => invoke<DirListing>('list_dir', { path })

/** Normalises and validates address-bar input; rejects non-directories. */
export const resolvePath = (input: string) => invoke<string>('resolve_path', { input })

export const parentPath = (path: string) => invoke<string | null>('parent_path', { path })

/** `[label, path]` pairs, outermost first. */
export const breadcrumbs = (path: string) =>
  invoke<[string, string][]>('breadcrumbs', { path })

export const homeDir = () => invoke<string>('home_dir')

export const createDir = (path: string) => invoke<void>('create_dir', { path })

export const renameEntry = (from: string, to: string) =>
  invoke<void>('rename_entry', { from, to })

// --- System clipboard (CF_HDROP on Windows) ---------------------------------

export const clipboardWriteFiles = (paths: string[], cut: boolean) =>
  invoke<void>('clipboard_write_files', { paths, cut })

export const clipboardReadFiles = () => invoke<ClipboardFiles>('clipboard_read_files')

export const clipboardHasFiles = () => invoke<boolean>('clipboard_has_files')

// --- Native shell context menu (third-party extensions) ----------------------

/**
 * Show the Windows shell menu at screen coordinates. Resolves once the
 * user picks something or dismisses it; `invoked` names the verb that
 * ran, so the caller knows whether to refresh.
 */
export const shellMenuShow = (paths: string[], x: number, y: number) =>
  invoke<ShellMenuResult>('shell_menu_show', { paths, x, y })

export const shellMenuList = (paths: string[]) =>
  invoke<ShellMenuResult>('shell_menu_list', { paths })

// --- Background operations ---------------------------------------------------

/** Queues the job and returns its id; progress arrives via `fileop:update`. */
export const startOperation = (
  kind: JobKind,
  sources: string[],
  destDir?: string,
  options?: JobOptions
) => invoke<string>('start_operation', { kind, sources, destDir, options })

// --- Archives ----------------------------------------------------------------

/** Extensions that open as an archive, straight from the Rust dispatch table. */
export const archiveExtensions = () => invoke<string[]>('archive_extensions')

/** Extracts one member to a scratch folder and returns the file's path. */
export const archiveOpenMember = (path: string) =>
  invoke<string>('archive_open_member', { path })

export const archiveSuggestName = (dir: string, sources: string[], extension: string) =>
  invoke<string>('archive_suggest_name', { dir, sources, extension })

export const cancelOperation = (id: string) => invoke<void>('cancel_operation', { id })

export const listOperations = () => invoke<JobState[]>('list_operations')

export const clearFinishedOperations = () => invoke<void>('clear_finished_operations')

export const listDrives = () => invoke<DriveInfo[]>('list_drives')

export const quickAccess = () => invoke<PlaceEntry[]>('quick_access')

export const listFavorites = () => invoke<Favorite[]>('list_favorites')

export const addFavorite = (path: string, name?: string, icon?: string) =>
  invoke<Favorite[]>('add_favorite', { path, name, icon })

export const removeFavorite = (id: string) => invoke<Favorite[]>('remove_favorite', { id })

export const reorderFavorites = (ids: string[]) =>
  invoke<Favorite[]>('reorder_favorites', { ids })

export const listRecent = () => invoke<RecentEntry[]>('list_recent')

export const recordVisit = (path: string) => invoke<void>('record_visit', { path })

export const clearRecent = () => invoke<void>('clear_recent')

// --- Persisted UI state (~/.config/shuttle-files/) ---------------------------

export const loadTabs = () => invoke<TabSnapshot[]>('load_tabs')

export const saveTabs = (tabs: TabSnapshot[]) => invoke<void>('save_tabs', { tabs })

export const loadViewSettings = () => invoke<ViewSettings>('load_view_settings')

export const saveViewSettings = (settings: ViewSettings) =>
  invoke<void>('save_view_settings', { settings })

export const loadOpenWith = () => invoke<OpenWithSettings>('load_open_with')

/** Resolves with the normalised settings; rejects if the program is missing. */
export const saveOpenWith = (settings: OpenWithSettings) =>
  invoke<OpenWithSettings>('save_open_with', { settings })

export const defaultOpenWith = () => invoke<OpenWithSettings>('default_open_with')

// --- Fuzzy search ------------------------------------------------------------

/**
 * Fuzzy-find inside `dir`. Reusing an `id` cancels the previous search
 * for it, which is what makes searching on every keystroke affordable.
 */
export const fuzzyFind = (
  id: string,
  dir: string,
  query: string,
  recursive: boolean,
  limit?: number
) => invoke<SearchResult>('fuzzy_find', { id, dir, query, recursive, limit })

export const cancelSearch = (id: string) => invoke<void>('cancel_search', { id })

// --- Checksums ---------------------------------------------------------------

/** Results stream back as `hash:result`, ending with `hash:finished`. */
export const startHash = (id: string, paths: string[], algos: HashAlgo[]) =>
  invoke<void>('start_hash', { id, paths, algos })

export const cancelHash = (id: string) => invoke<void>('cancel_hash', { id })
