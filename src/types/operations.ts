export type JobKind = 'copy' | 'move' | 'delete' | 'extract' | 'compress'

/** Extra input for the archive jobs. */
export interface JobOptions {
  /** Compress: full path of the archive to create; its extension picks the format. */
  archivePath?: string
  /** Compress: 0 stores, 9 compresses hardest. */
  level?: number
  /**
   * Copy/Move: replace an existing destination instead of auto-renaming
   * (`report.txt` -> `report (2).txt`). Set once the user has confirmed
   * Overwrite in the conflict dialog.
   */
  overwrite?: boolean
}

export type JobStatus = 'scanning' | 'running' | 'completed' | 'failed' | 'cancelled'

/** Snapshot pushed from Rust on every `fileop:update` event. */
export interface JobState {
  id: string
  kind: JobKind
  status: JobStatus
  label: string
  destDir: string
  totalFiles: number
  doneFiles: number
  totalBytes: number
  doneBytes: number
  current: string
  error: string
  bytesPerSec: number
}

export interface ClipboardFiles {
  paths: string[]
  cut: boolean
}

/** One entry of the native Windows shell context menu. */
export interface ShellMenuItem {
  /** Command id; null for separators and pure submenus. */
  id: number | null
  label: string
  /** Language-independent verb, when the shell exposes one. */
  verb: string
  separator: boolean
  enabled: boolean
  default: boolean
  /**
   * Whether the shell attached a submenu. `children` can still be empty:
   * several extensions build their submenu lazily, only once the menu is
   * actually displayed.
   */
  hasSubmenu: boolean
  children: ShellMenuItem[]
}

export interface ShellMenuResult {
  items: ShellMenuItem[]
  /** Verb the user picked; empty when the menu was dismissed. */
  invoked: string
}

export const FINISHED_STATUSES: JobStatus[] = ['completed', 'failed', 'cancelled']
