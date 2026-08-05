import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { ROOT, type TabSnapshot } from '@/types/filesystem'
import { loadTabs, recordVisit, saveTabs } from '@/composables/useTauri'
import { isInsideArchive, splitArchive } from '@/stores/archives'

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
  /**
   * Fuzzy-find query for this tab. Per-tab rather than app-wide: a new
   * tab must start clean even when it opens the folder the search was
   * run in, and coming back to a tab should restore what you were
   * looking at. Session-scoped, like `history`.
   */
  filter: string
  /** Whether this tab's query descends into subfolders. */
  recursive: boolean
}

/**
 * One side of the window: its own tabs, its own front tab.
 *
 * A pane rather than a second window because the point of the split is
 * comparing and moving between two folders — copy, move and drag all
 * work across it, and one toolbar and one status line follow whichever
 * side has the focus.
 */
export interface Pane {
  id: string
  tabs: Tab[]
  activeTabId: string
}

/** The window is never paneless; two is as many as the split allows. */
export const MAX_PANES = 2

/** Legacy localStorage key, read once so existing users keep their tabs. */
const LEGACY_STORAGE_KEY = 'shuttle-files:tabs'

/** True when a soft-locked tab has been browsed away from its base folder. */
export function hasStrayed(tab: Tab): boolean {
  return tab.lock === 'locked-allow-dirs' && tab.path !== tab.lockedPath
}

function titleFor(path: string): string {
  if (path === ROOT) return 'This PC'
  // An archive's root ends with the marker, whose separator would
  // otherwise be trimmed into a stray "!".
  const inside = splitArchive(path)
  if (inside && !inside.inner) return titleFor(inside.archive)
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
    filter: '',
    recursive: false,
  }
}

function asLock(value: unknown): TabLock {
  return value === 'locked' || value === 'locked-allow-dirs' ? value : 'none'
}

function makePane(tabs: Tab[]): Pane {
  return { id: crypto.randomUUID(), tabs, activeTabId: tabs[0]?.id ?? '' }
}

/** A tab as it was stored, with the side of the split it was on. */
interface RestoredTab {
  tab: Tab
  pane: number
  active: boolean
}

/** Rebuild a tab from a persisted snapshot, tolerating older shapes. */
function tabFromSnapshot(item: unknown): RestoredTab {
  // The very first builds stored a plain array of paths.
  if (typeof item === 'string') return { tab: makeTab(item), pane: 0, active: false }
  if (!item || typeof item !== 'object') return { tab: makeTab(), pane: 0, active: false }
  const { path, lock, lockedPath, pane, active } = item as Record<string, unknown>
  const mode = asLock(lock)
  const base = typeof lockedPath === 'string' ? lockedPath : ROOT
  // A locked tab always comes back on its base folder.
  const start = mode === 'none' ? (typeof path === 'string' ? path : ROOT) : base
  return {
    tab: makeTab(start, mode, base),
    // Snapshots written before the split existed have no side, which is
    // the single pane they were saved from.
    pane: typeof pane === 'number' && pane >= 1 ? 1 : 0,
    active: active === true,
  }
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
  const panes = ref<Pane[]>([])
  const activePaneId = ref('')

  const split = computed(() => panes.value.length > 1)
  const activePane = computed(
    () => panes.value.find((p) => p.id === activePaneId.value) ?? panes.value[0] ?? null
  )

  /** The focused pane's tabs — what the toolbar and the shortcuts act on. */
  const tabs = computed(() => activePane.value?.tabs ?? [])
  const activeTabId = computed(() => activePane.value?.activeTabId || null)
  const activeTab = computed(() => findTab(activeTabId.value))

  const canGoBack = computed(() => {
    const tab = activeTab.value
    return !!tab && tab.lock !== 'locked' && tab.historyIndex > 0
  })
  const canGoForward = computed(() => {
    const tab = activeTab.value
    return !!tab && tab.lock !== 'locked' && tab.historyIndex < tab.history.length - 1
  })

  /** Tab ids are unique across the window, so a lookup need not say where. */
  function findTab(id: string | null): Tab | null {
    if (!id) return null
    for (const pane of panes.value) {
      const tab = pane.tabs.find((t) => t.id === id)
      if (tab) return tab
    }
    return null
  }

  function paneOf(tabId: string): Pane | null {
    return panes.value.find((p) => p.tabs.some((t) => t.id === tabId)) ?? null
  }

  function paneById(id: string): Pane | null {
    return panes.value.find((p) => p.id === id) ?? null
  }

  /** The window always has a pane to put a tab in, even at startup. */
  function ensurePane(): Pane {
    if (panes.value.length === 0) {
      const pane = makePane([])
      panes.value.push(pane)
    }
    if (!paneById(activePaneId.value)) activePaneId.value = panes.value[0]!.id
    return paneById(activePaneId.value)!
  }

  function persist() {
    // Only the paths, lock state and layout are worth restoring; history
    // is intentionally session-scoped.
    const snapshot: TabSnapshot[] = panes.value.flatMap((pane, side) =>
      pane.tabs.map((t) => ({
        path: t.path,
        lock: t.lock,
        lockedPath: t.lockedPath,
        pane: side,
        active: t.id === pane.activeTabId,
      }))
    )
    saveTabs(snapshot).catch((e) => console.error('Cannot save tabs:', e))
  }

  async function restore() {
    try {
      const legacy = takeLegacySnapshot()
      const saved = legacy ?? (await loadTabs())
      if (saved.length > 0) {
        const restored = saved.map(tabFromSnapshot)
        // A side with nothing on it is not a side: a snapshot that only
        // ever filled the right pane comes back as a single one.
        panes.value = [0, 1]
          .map((side) => restored.filter((r) => r.pane === side))
          .filter((group) => group.length > 0)
          .map((group) => {
            const pane = makePane(group.map((r) => r.tab))
            pane.activeTabId = (group.find((r) => r.active) ?? group[0]!).tab.id
            return pane
          })
        activePaneId.value = panes.value[0]!.id
        // Writing back turns a migrated localStorage snapshot into tabs.json.
        if (legacy) persist()
        return
      }
    } catch (e) {
      // Corrupt or unreadable state must never block startup.
      console.error('Cannot restore tabs:', e)
      panes.value = []
    }
    addTab()
  }

  function addTab(path = ROOT, activate = true): Tab {
    return addTabIn(ensurePane().id, path, activate)
  }

  /** Opening a tab from a pane keeps it on that pane's side. */
  function addTabIn(paneId: string, path = ROOT, activate = true): Tab {
    const pane = paneById(paneId) ?? ensurePane()
    const tab = makeTab(path)
    pane.tabs.push(tab)
    if (activate) {
      pane.activeTabId = tab.id
      activePaneId.value = pane.id
    }
    persist()
    return tab
  }

  /** Locked tabs resist casual closing; `force` comes from explicit menu actions. */
  function closeTab(id: string, force = false) {
    const pane = paneOf(id)
    if (!pane) return
    const index = pane.tabs.findIndex((t) => t.id === id)
    if (index === -1) return
    if (!force && pane.tabs[index]!.lock !== 'none') return
    pane.tabs.splice(index, 1)
    if (pane.tabs.length === 0) {
      // Emptying one side of a split closes that side. Emptying the only
      // pane leaves nothing to show, so a fresh tab takes its place.
      if (split.value) closePane(pane.id)
      else addTabIn(pane.id)
      return
    }
    if (pane.activeTabId === id) {
      // Focus the neighbour on the left, like browsers do.
      pane.activeTabId = pane.tabs[Math.max(0, index - 1)]!.id
    }
    persist()
  }

  function closeOthers(id: string) {
    const pane = paneOf(id)
    if (!pane) return
    // Locked tabs survive, matching Total Commander's "close all tabs".
    pane.tabs = pane.tabs.filter((t) => t.id === id || t.lock !== 'none')
    pane.activeTabId = id
    activePaneId.value = pane.id
    persist()
  }

  function duplicateTab(id: string) {
    const pane = paneOf(id)
    const tab = findTab(id)
    if (pane && tab) addTabIn(pane.id, tab.path)
  }

  function setLock(id: string, mode: TabLock) {
    const tab = findTab(id)
    if (!tab) return
    tab.lock = mode
    // The folder in view when locking becomes the tab's base folder.
    tab.lockedPath = mode === 'none' ? ROOT : tab.path
    persist()
  }

  function setFilter(id: string, value: string) {
    const tab = findTab(id)
    if (tab) tab.filter = value
  }

  function setRecursive(id: string, value: boolean) {
    const tab = findTab(id)
    if (tab) tab.recursive = value
  }

  function setActiveTab(id: string) {
    const pane = paneOf(id)
    const tab = findTab(id)
    if (!pane || !tab) return
    pane.activeTabId = id
    activePaneId.value = pane.id
    // Coming back to a soft-locked tab snaps it to its base folder.
    if (hasStrayed(tab)) pushHistory(tab, tab.lockedPath)
    persist()
  }

  /** Reorder after a tab drag; a drag never leaves its own tab bar. */
  function moveTab(paneId: string, from: number, to: number) {
    const pane = paneById(paneId)
    if (!pane || from === to) return
    const [moved] = pane.tabs.splice(from, 1)
    if (moved) pane.tabs.splice(to, 0, moved)
    persist()
  }

  function setActivePane(id: string) {
    if (paneById(id)) activePaneId.value = id
  }

  /** Alternate sides, which is all there is to do with two of them. */
  function focusOtherPane() {
    if (!split.value) return
    const index = panes.value.findIndex((p) => p.id === activePaneId.value)
    activePaneId.value = panes.value[(index + 1) % panes.value.length]!.id
  }

  /**
   * Closing a side folds its tabs into the one that stays rather than
   * dropping them: a split is a way of arranging tabs, not a second
   * place to lose them.
   */
  function closePane(id: string) {
    if (!split.value) return
    const closing = paneById(id)
    const keep = panes.value.find((p) => p.id !== id)
    if (!closing || !keep) return
    keep.tabs.push(...closing.tabs)
    panes.value = panes.value.filter((p) => p.id !== id)
    if (!paneById(activePaneId.value)) activePaneId.value = keep.id
    persist()
  }

  /**
   * Splitting opens the new side on the folder already in view: two
   * copies of where you are is one navigation away from any comparison,
   * and never a surprise. Unsplitting keeps the side being worked in,
   * and the other side's tabs come with it.
   */
  function toggleSplit() {
    if (split.value) {
      closePane(panes.value.find((p) => p.id !== activePaneId.value)!.id)
      return
    }
    const source = ensurePane()
    const pane = makePane([makeTab(findTab(source.activeTabId)?.path ?? ROOT)])
    panes.value.push(pane)
    activePaneId.value = pane.id
    persist()
  }

  /** Sends a tab across the split, which is how a comparison is set up. */
  function moveTabToOtherPane(id: string) {
    if (!split.value) return
    const from = paneOf(id)
    const tab = findTab(id)
    if (!from || !tab) return
    const to = panes.value.find((p) => p.id !== from.id)!
    from.tabs = from.tabs.filter((t) => t.id !== id)
    to.tabs.push(tab)
    to.activeTabId = id
    activePaneId.value = to.id
    // The side it left may have been holding nothing else.
    if (from.tabs.length === 0) closePane(from.id)
    else if (from.activeTabId === id) from.activeTabId = from.tabs[from.tabs.length - 1]!.id
    persist()
  }

  function applyPath(tab: Tab, path: string) {
    tab.path = path
    tab.title = titleFor(path)
    // A query is about the folder it was typed in; carrying it into the
    // next one would silently hide most of what is there.
    tab.filter = ''
    persist()
    // Paths inside an archive are not places on disk, so they stay out
    // of the visit history the fast dial ranks.
    if (path !== ROOT && !isInsideArchive(path)) {
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
    const tab = findTab(id)
    if (!tab || tab.path === path) return
    // A hard-locked tab never leaves its folder; the target opens in a
    // new tab beside it, on the same side of the split.
    if (tab.lock === 'locked') {
      addTabIn(paneOf(id)?.id ?? ensurePane().id, path)
      return
    }
    pushHistory(tab, path)
  }

  function goBack(id: string) {
    const tab = findTab(id)
    if (!tab || tab.lock === 'locked' || tab.historyIndex <= 0) return
    tab.historyIndex--
    applyPath(tab, tab.history[tab.historyIndex]!)
  }

  function goForward(id: string) {
    const tab = findTab(id)
    if (!tab || tab.lock === 'locked' || tab.historyIndex >= tab.history.length - 1) return
    tab.historyIndex++
    applyPath(tab, tab.history[tab.historyIndex]!)
  }

  return {
    panes,
    activePaneId,
    activePane,
    split,
    tabs,
    activeTabId,
    activeTab,
    canGoBack,
    canGoForward,
    restore,
    addTab,
    addTabIn,
    closeTab,
    closeOthers,
    duplicateTab,
    setActiveTab,
    setActivePane,
    setLock,
    setFilter,
    setRecursive,
    moveTab,
    moveTabToOtherPane,
    focusOtherPane,
    toggleSplit,
    navigate,
    goBack,
    goForward,
  }
})
