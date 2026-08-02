export interface FileEntry {
  name: string
  path: string
  isDir: boolean
  isSymlink: boolean
  isHidden: boolean
  size: number
  /** Unix seconds; 0 when unavailable. */
  modified: number
  /** Lowercase extension without the dot. */
  ext: string
}

export interface DirListing {
  path: string
  displayName: string
  parent: string | null
  isVirtualRoot: boolean
  entries: FileEntry[]
}

export type DriveKind = 'fixed' | 'removable' | 'network' | 'cdrom' | 'ramdisk' | 'unknown'

export interface DriveInfo {
  path: string
  label: string
  kind: DriveKind
  totalBytes: number
  freeBytes: number
}

export interface PlaceEntry {
  name: string
  path: string
  icon: string
}

export interface Favorite {
  id: string
  name: string
  path: string
  icon: string
}

export interface RecentEntry {
  path: string
  name: string
  visits: number
  lastVisit: number
}

/** The virtual root ("This PC"), which renders as the fast dial. */
export const ROOT = ''

/** Persisted shape of a tab in `~/.config/shuttle-files/tabs.json`. */
export interface TabSnapshot {
  path: string
  lock: 'none' | 'locked' | 'locked-allow-dirs'
  lockedPath: string
}

/** Persisted shape of `~/.config/shuttle-files/view.json`. */
export interface ViewSettings {
  rowScale: number
}
