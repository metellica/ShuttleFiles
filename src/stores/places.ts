import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import * as api from '@/composables/useTauri'
import type { DriveInfo, Favorite, PlaceEntry, RecentEntry } from '@/types/filesystem'

/** Drives, well-known folders, favorites and history — the fast dial's data. */
export const usePlacesStore = defineStore('places', () => {
  const drives = ref<DriveInfo[]>([])
  const quickAccess = ref<PlaceEntry[]>([])
  const favorites = ref<Favorite[]>([])
  const recent = ref<RecentEntry[]>([])

  /** Most-visited folders, the fast dial's third row. */
  const frequent = computed(() =>
    [...recent.value].sort((a, b) => b.visits - a.visits).slice(0, 12)
  )

  function isFavorite(path: string) {
    return favorites.value.some((f) => f.path === path)
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
    await api.clearRecent()
    recent.value = []
  }

  return {
    drives,
    quickAccess,
    favorites,
    recent,
    frequent,
    isFavorite,
    refresh,
    toggleFavorite,
    remove,
    reorder,
    clearRecent,
  }
})
