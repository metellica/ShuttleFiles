import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import * as api from '@/composables/useTauri'
import type { DriveInfo, Favorite, PlaceEntry, RecentEntry } from '@/types/filesystem'

/** Folders shown per machine on the fast dial. */
const GROUP_PREVIEW = 6
/** A remote untouched this long is folded away on the fast dial. */
const DORMANT_SECS = 14 * 86_400

export interface RecentGroup {
  host: string
  label: string
  items: RecentEntry[]
  total: number
  lastVisit: number
  dormant: boolean
}

/** Drives, well-known folders, favorites and history — the fast dial's data. */
export const usePlacesStore = defineStore('places', () => {
  const drives = ref<DriveInfo[]>([])
  const quickAccess = ref<PlaceEntry[]>([])
  const favorites = ref<Favorite[]>([])
  const recent = ref<RecentEntry[]>([])

  /**
   * History grouped by machine, the fast dial's "Recent" cards. Local
   * first, then remotes by how recently any of their folders was used.
   */
  const recentGroups = computed<RecentGroup[]>(() => {
    const byHost = new Map<string, RecentEntry[]>()
    for (const entry of recent.value) {
      const list = byHost.get(entry.host)
      if (list) list.push(entry)
      else byHost.set(entry.host, [entry])
    }
    const now = Date.now() / 1000
    const groups = [...byHost.entries()].map(([host, entries]) => {
      const lastVisit = Math.max(...entries.map((e) => e.lastVisit))
      return {
        host,
        label: host || 'Local',
        // `recent` arrives ranked, so the head is already the best.
        items: entries.slice(0, GROUP_PREVIEW),
        total: entries.length,
        lastVisit,
        dormant: host !== '' && now - lastVisit > DORMANT_SECS,
      }
    })
    return groups.sort((a, b) => {
      if (!a.host !== !b.host) return a.host ? 1 : -1
      return b.lastVisit - a.lastVisit
    })
  })

  function isFavorite(path: string) {
    return favorites.value.some((f) => f.path === path)
  }

  async function refreshRecent() {
    recent.value = await api.listRecent()
  }

  async function togglePin(path: string) {
    recent.value = await api.togglePinRecent(path)
  }

  async function removeRecent(path: string) {
    recent.value = await api.removeRecent(path)
  }

  async function refresh() {
    const [d, q, f, r] = await Promise.all([
      api.listDrives(),
      api.quickAccess(),
      api.listFavorites(),
      api.listRecent(),
    ])
    drives.value = d
    quickAccess.value = q
    favorites.value = f
    recent.value = r
  }

  async function toggleFavorite(path: string, name?: string) {
    const existing = favorites.value.find((f) => f.path === path)
    favorites.value = existing
      ? await api.removeFavorite(existing.id)
      : await api.addFavorite(path, name)
  }

  async function remove(id: string) {
    favorites.value = await api.removeFavorite(id)
  }

  async function reorder(ids: string[]) {
    favorites.value = await api.reorderFavorites(ids)
  }

  async function clearRecent() {
    recent.value = await api.clearRecent()
  }

  return {
    drives,
    quickAccess,
    favorites,
    recent,
    recentGroups,
    isFavorite,
    refresh,
    refreshRecent,
    togglePin,
    removeRecent,
    toggleFavorite,
    remove,
    reorder,
    clearRecent,
  }
})
