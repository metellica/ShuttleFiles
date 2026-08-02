import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { ROOT, type TabSnapshot } from '@/types/filesystem'
import { loadTabs, recordVisit, saveTabs } from '@/composables/useTauri'

/**
 * Tab locking, modelled on Total Commander:
 * - `none`: an ordinary tab.
 * - `locked`: the folder is fixed. Navigating elsewhere opens a new tab.
 * - `locked-allow-dirs`: browsing is allowed, but returning to the tab
 *   restores its base folder.
 */
export type TabLock = 'none' | 'locked' | 'locked-allow-dirs'

export interface Tab {
  id: string
  /** Current directory; ROOT ('') renders the fast dial. */
  path: string
  title: string
  /** Visited paths for Back/Forward, newest last. */
  history: string[]
  historyIndex: number
  lock: TabLock
  /** Base folder a locked tab is pinned to; meaningless when unlocked. */
  lockedPath: string
}

/** Legacy localStorage key, read once so existing users keep their tabs. */
const LEGACY_STORAGE_KEY = 'shuttle-files:tabs'

/** True when a soft-locked tab has been browsed away from its base folder. */
export function hasStrayed(tab: Tab): boolean {
  return tab.lock === 'locked-allow-dirs' && tab.path !== tab.lockedPath
}

function titleFor(path: string): string {
  if (path === ROOT) return 'This PC'
  const trimmed = path.replace(/[\\/]+$/, '')
  const idx = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'))
  return idx >= 0 ? trimmed.slice(idx + 1) || trimmed : trimmed
}

function makeTab(path = ROOT, lock: TabLock = 'none', lockedPath = path): Tab {
  return {
    id: crypto.randomUUID(),
    path,
    title: titleFor(path),
    history: [path],
    historyIndex: 0,
    lock,
    lockedPath: lock === 'none' ? ROOT : lockedPath,
  }
}

function asLock(value: unknown): TabLock {
  return value === 'locked' || value === 'locked-allow-dirs' ? value : 'none'
}

/** Rebuild a tab from a persisted snapshot, tolerating older shapes. */
function tabFromSnapshot(item: unknown): Tab {
  // The very first builds stored a plain array of paths.
  if (typeof item === 'string') return makeTab(item)
  if (!item || typeof item !== 'object') return makeTab()
  const { path, lock, lockedPath } = item as Record<string, unknown>
  const mode = asLock(lock)
  const base = typeof lockedPath === 'string' ? lockedPath : ROOT
  // A locked tab always comes back on its base folder.
  const start = mode === 'none' ? (typeof path === 'string' ? path : ROOT) : base
  return makeTab(start, mode, base)
}

/** Tabs saved by an older build that used the WebView's localStorage. */
function takeLegacySnapshot(): unknown[] | null {
  try {
    const raw = localStorage.getItem(LEGACY_STORAGE_KEY)
    if (!raw) return null
    localStorage.removeItem(LEGACY_STORAGE_KEY)
    const parsed: unknown = JSON.parse(raw)
    return Array.isArray(parsed) && parsed.length > 0 ? parsed : null
  } catch {
    localStorage.removeItem(LEGACY_STORAGE_KEY)
    return null
  }
}

export const useTabsStore = defineStore('tabs', () => {
  const tabs = ref<Tab[]>([])
  const activeTabId = ref<string | null>(null)

  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null)
  const canGoBack = computed(() => {
    const tab = activeTab.value
    return !!tab && tab.lock !== 'locked' && tab.historyIndex > 0
  })
  const canGoForward = computed(() => {
    const tab = activeTab.value
    return !!tab && tab.lock !== 'locked' && tab.historyIndex < tab.history.length - 1
  })

  function persist() {
    // Only the paths and lock state are worth restoring; history is
    // intentionally session-scoped.
    const snapshot: TabSnapshot[] = tabs.value.map((t) => ({
      path: t.path,
      lock: t.lock,
      lockedPath: t.lockedPath,
    }))
    saveTabs(snapshot).catch((e) => console.error('Cannot save tabs:', e))
  }

  async function restore() {
    try {
      const legacy = takeLegacySnapshot()
      const saved = legacy ?? (await loadTabs())
      if (saved.length > 0) {
        tabs.value = saved.map(tabFromSnapshot)
        activeTabId.value = tabs.value[0]!.id
        // Writing back turns a migrated localStorage snapshot into tabs.json.
        if (legacy) persist()
        return
      }
    } catch (e) {
      // Corrupt or unreadable state must never block startup.
      console.error('Cannot restore tabs:', e)
    }
    addTab()
  }

  function addTab(path = ROOT, activate = true): Tab {
    const tab = makeTab(path)
    tabs.value.push(tab)
    if (activate) activeTabId.value = tab.id
    persist()
    return tab
  }

  /** Locked tabs resist casual closing; `force` comes from explicit menu actions. */
  function closeTab(id: string, force = false) {
    const index = tabs.value.findIndex((t) => t.id === id)
    if (index === -1) return
    if (!force && tabs.value[index]!.lock !== 'none') return
    tabs.value.splice(index, 1)
    if (tabs.value.length === 0) {
      addTab()
      return
    }
    if (activeTabId.value === id) {
      // Focus the neighbour on the left, like browsers do.
      activeTabId.value = tabs.value[Math.max(0, index - 1)]!.id
    }
    persist()
  }

  function closeOthers(id: string) {
    // Locked tabs survive, matching Total Commander's "close all tabs".
    tabs.value = tabs.value.filter((t) => t.id === id || t.lock !== 'none')
    activeTabId.value = id
    persist()
  }

  function duplicateTab(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (tab) addTab(tab.path)
  }

  function setLock(id: string, mode: TabLock) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab) return
    tab.lock = mode
    // The folder in view when locking becomes the tab's base folder.
    tab.lockedPath = mode === 'none' ? ROOT : tab.path
    persist()
  }

  function setActiveTab(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab) return
    activeTabId.value = id
    // Coming back to a soft-locked tab snaps it to its base folder.
    if (hasStrayed(tab)) pushHistory(tab, tab.lockedPath)
  }

  /** Reorder after a tab drag. */
  function moveTab(from: number, to: number) {
    if (from === to) return
    const [moved] = tabs.value.splice(from, 1)
    if (moved) tabs.value.splice(to, 0, moved)
    persist()
  }

  function applyPath(tab: Tab, path: string) {
    tab.path = path
    tab.title = titleFor(path)
    persist()
    if (path !== ROOT) {
      recordVisit(path).catch(() => {
        // History is a convenience; a failed write must not break navigation.
      })
    }
  }

  function pushHistory(tab: Tab, path: string) {
    if (tab.path === path) return
    // A new destination truncates any forward history, as in a browser.
    tab.history = tab.history.slice(0, tab.historyIndex + 1)
    tab.history.push(path)
    tab.historyIndex = tab.history.length - 1
    applyPath(tab, path)
  }

  function navigate(id: string, path: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.path === path) return
    // A hard-locked tab never leaves its folder; the target opens in a new tab.
    if (tab.lock === 'locked') {
      addTab(path)
      return
    }
    pushHistory(tab, path)
  }

  function goBack(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.lock === 'locked' || tab.historyIndex <= 0) return
    tab.historyIndex--
    applyPath(tab, tab.history[tab.historyIndex]!)
  }

  function goForward(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.lock === 'locked' || tab.historyIndex >= tab.history.length - 1) return
    tab.historyIndex++
    applyPath(tab, tab.history[tab.historyIndex]!)
  }

  return {
    tabs,
    activeTabId,
    activeTab,
    canGoBack,
    canGoForward,
    restore,
    addTab,
    closeTab,
    closeOthers,
    duplicateTab,
    setActiveTab,
    setLock,
    moveTab,
    navigate,
    goBack,
    goForward,
  }
})
