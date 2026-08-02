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

/**
 * A fuzzy-search match. The entry's fields are flattened in, so a hit can
 * be rendered by the file list without unwrapping.
 */
export interface SearchHit extends FileEntry {
  /** Path relative to the search root; equal to `name` when not recursive. */
  rel: string
  score: number
  /** Char indices into `rel` that matched, for highlighting. */
  positions: number[]
}

export interface SearchResult {
  hits: SearchHit[]
  /** Total matches found, which may exceed `hits.length`. */
  total: number
  scanned: number
  truncated: boolean
  cancelled: boolean
}

export type HashAlgo = 'md5' | 'sha256'

export interface HashResult {
  path: string
  name: string
  size: number
  md5: string
  sha256: string
  /** Empty on success. */
  error: string
}

export interface HashProgress {
  id: string
  path: string
  /** 1-based position in the batch. */
  index: number
  total: number
  doneBytes: number
  totalBytes: number
}
