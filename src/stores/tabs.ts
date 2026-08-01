import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { ROOT } from '@/types/filesystem'
import { recordVisit } from '@/composables/useTauri'

export interface Tab {
  id: string
  /** Current directory; ROOT ('') renders the fast dial. */
  path: string
  title: string
  /** Visited paths for Back/Forward, newest last. */
  history: string[]
  historyIndex: number
}

const STORAGE_KEY = 'shuttle-files:tabs'

function titleFor(path: string): string {
  if (path === ROOT) return 'This PC'
  const trimmed = path.replace(/[\\/]+$/, '')
  const idx = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'))
  return idx >= 0 ? trimmed.slice(idx + 1) || trimmed : trimmed
}

function makeTab(path = ROOT): Tab {
  return {
    id: crypto.randomUUID(),
    path,
    title: titleFor(path),
    history: [path],
    historyIndex: 0,
  }
}

export const useTabsStore = defineStore('tabs', () => {
  const tabs = ref<Tab[]>([])
  const activeTabId = ref<string | null>(null)

  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null)
  const canGoBack = computed(() => (activeTab.value?.historyIndex ?? 0) > 0)
  const canGoForward = computed(() => {
    const tab = activeTab.value
    return !!tab && tab.historyIndex < tab.history.length - 1
  })

  function persist() {
    // Only the paths are worth restoring; history is intentionally session-scoped.
    const snapshot = tabs.value.map((t) => t.path)
    localStorage.setItem(STORAGE_KEY, JSON.stringify(snapshot))
  }

  function restore() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY)
      const paths: unknown = raw ? JSON.parse(raw) : null
      if (Array.isArray(paths) && paths.length > 0) {
        tabs.value = paths.map((p) => makeTab(typeof p === 'string' ? p : ROOT))
        activeTabId.value = tabs.value[0]!.id
        return
      }
    } catch {
      // Corrupt state must never block startup.
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

  function closeTab(id: string) {
    const index = tabs.value.findIndex((t) => t.id === id)
    if (index === -1) return
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
    tabs.value = tabs.value.filter((t) => t.id === id)
    activeTabId.value = id
    persist()
  }

  function duplicateTab(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (tab) addTab(tab.path)
  }

  function setActiveTab(id: string) {
    activeTabId.value = id
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

  function navigate(id: string, path: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.path === path) return
    // A new destination truncates any forward history, as in a browser.
    tab.history = tab.history.slice(0, tab.historyIndex + 1)
    tab.history.push(path)
    tab.historyIndex = tab.history.length - 1
    applyPath(tab, path)
  }

  function goBack(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.historyIndex <= 0) return
    tab.historyIndex--
    applyPath(tab, tab.history[tab.historyIndex]!)
  }

  function goForward(id: string) {
    const tab = tabs.value.find((t) => t.id === id)
    if (!tab || tab.historyIndex >= tab.history.length - 1) return
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
    moveTab,
    navigate,
    goBack,
    goForward,
  }
})
