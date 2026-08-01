import { invoke } from '@tauri-apps/api/core'
import type {
  DirListing,
  DriveInfo,
  Favorite,
  PlaceEntry,
  RecentEntry,
} from '@/types/filesystem'
import type {
  ClipboardFiles,
  JobKind,
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
export const startOperation = (kind: JobKind, sources: string[], destDir?: string) =>
  invoke<string>('start_operation', { kind, sources, destDir })

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
