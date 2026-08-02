import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { FileEntry } from '@/types/filesystem'
import { archiveExtensions } from '@/composables/useTauri'

/**
 * Marks the boundary between an archive's path and a path inside it —
 * `D:\pkg.zip!\docs\readme.md`. Mirrors `fs::path::ARCHIVE_MARK`.
 */
export const ARCHIVE_MARK = '!\\'

/** Split a virtual path into the archive and the path inside it. */
export function splitArchive(path: string): { archive: string; inner: string } | null {
  const index = path.indexOf(ARCHIVE_MARK)
  if (index === -1) return null
  return {
    archive: path.slice(0, index),
    inner: path.slice(index + ARCHIVE_MARK.length).replace(/^[\\/]+|[\\/]+$/g, ''),
  }
}

export function isInsideArchive(path: string): boolean {
  return path.includes(ARCHIVE_MARK)
}

/** The virtual path of an archive's root, which is what navigation enters. */
export function archiveRoot(archive: string): string {
  return `${archive}${ARCHIVE_MARK}`
}

/** Formats offered when creating an archive, best default first. */
export const CREATABLE_FORMATS = [
  { extension: 'zip', label: 'ZIP' },
  { extension: '7z', label: '7z (LZMA2)' },
  { extension: 'tar.gz', label: 'TAR + gzip' },
  { extension: 'tar.zst', label: 'TAR + zstd' },
  { extension: 'tar.xz', label: 'TAR + xz' },
  { extension: 'tar.bz2', label: 'TAR + bzip2' },
  { extension: 'tar', label: 'TAR (no compression)' },
]

export const COMPRESSION_LEVELS = [
  { value: 0, label: 'Store' },
  { value: 3, label: 'Fast' },
  { value: 6, label: 'Normal' },
  { value: 9, label: 'Maximum' },
]

/**
 * Which files open as archives. The list comes from Rust so the two
 * sides cannot disagree about what is browsable.
 */
export const useArchivesStore = defineStore('archives', () => {
  const extensions = ref<string[]>([])

  const extensionSet = computed(() => new Set(extensions.value))

  async function restore() {
    try {
      extensions.value = await archiveExtensions()
    } catch (e) {
      console.error('Cannot load archive formats:', e)
    }
  }

  /** Matches compound extensions too, so `.tar.gz` beats `.gz`. */
  function isArchiveFile(entry: FileEntry): boolean {
    if (entry.isDir) return false
    const name = entry.name.toLowerCase()
    return [...extensionSet.value].some((ext) => name.endsWith(`.${ext}`))
  }

  return { extensions, restore, isArchiveFile }
})
