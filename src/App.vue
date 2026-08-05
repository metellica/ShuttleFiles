<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { useTabsStore, type Pane, type Tab } from '@/stores/tabs'
import { usePlacesStore } from '@/stores/places'
import { useClipboardStore } from '@/stores/clipboard'
import { useOperationsStore } from '@/stores/operations'
import { useViewSettingsStore } from '@/stores/viewSettings'
import { useOpenWithStore } from '@/stores/openWith'
import { isInsideArchive, useArchivesStore } from '@/stores/archives'
import { parentPath } from '@/composables/useTauri'
import { ROOT } from '@/types/filesystem'
import TabBar from '@/components/layout/TabBar.vue'
import Toolbar from '@/components/layout/Toolbar.vue'
import OperationBar from '@/components/layout/OperationBar.vue'
import FileBrowser from '@/components/browser/FileBrowser.vue'
import PromptDialog from '@/components/common/PromptDialog.vue'
import SettingsDialog from '@/components/common/SettingsDialog.vue'

type Browser = InstanceType<typeof FileBrowser>
interface PaneStats {
  total: number
  selected: number
  searching: boolean
  truncated: boolean
}

const tabs = useTabsStore()
const places = usePlacesStore()
const clipboard = useClipboardStore()
const ops = useOperationsStore()
const view = useViewSettingsStore()
const openWith = useOpenWithStore()
const archives = useArchivesStore()

let unlistenOps: UnlistenFn | null = null

const EMPTY_STATS: PaneStats = { total: 0, selected: 0, searching: false, truncated: false }
const paneStats = ref<Record<string, PaneStats>>({})
const upTarget = ref<string | null>(null)
const toolbarRef = ref<InstanceType<typeof Toolbar> | null>(null)
const contentRef = ref<HTMLElement | null>(null)
const settingsOpen = ref(false)

/**
 * One browser per pane, reached by pane id rather than a single ref, so
 * a shortcut always lands on the side that has the focus. Kept out of
 * the reactive graph: component instances are only ever called, never
 * rendered.
 */
const browsers = new Map<string, Browser>()

function setBrowser(paneId: string, el: unknown) {
  if (el) browsers.set(paneId, el as Browser)
}

function activeBrowser(): Browser | null {
  return browsers.get(tabs.activePaneId) ?? null
}

function paneTab(pane: Pane): Tab | null {
  return pane.tabs.find((t) => t.id === pane.activeTabId) ?? null
}

const currentPath = computed(() => tabs.activeTab?.path ?? ROOT)
const stats = computed(() => paneStats.value[tabs.activePaneId] ?? EMPTY_STATS)

/** Both rows of the split — the tab bars and the panes — share a grid. */
const columns = computed(() =>
  tabs.split ? `${view.splitRatio}fr 5px ${1 - view.splitRatio}fr` : '1fr'
)

/** An archive's contents cannot be written to, so those actions go away. */
const readOnlyLocation = computed(() => isInsideArchive(currentPath.value))

// The query belongs to the tab, so switching tabs restores what that tab
// was looking at and a new tab always starts clean.
const filter = computed({
  get: () => tabs.activeTab?.filter ?? '',
  set: (value: string) => {
    if (tabs.activeTabId) tabs.setFilter(tabs.activeTabId, value)
  },
})

const recursive = computed({
  get: () => tabs.activeTab?.recursive ?? false,
  set: (value: boolean) => {
    if (tabs.activeTabId) tabs.setRecursive(tabs.activeTabId, value)
  },
})

watch(currentPath, async (path) => {
  upTarget.value = path === ROOT ? null : await parentPath(path)
}, { immediate: true })

// A closed pane leaves a browser and a row of counters behind it.
watch(
  () => tabs.panes.map((p) => p.id),
  (ids) => {
    for (const id of [...browsers.keys()]) if (!ids.includes(id)) browsers.delete(id)
    for (const id of Object.keys(paneStats.value)) {
      if (!ids.includes(id)) delete paneStats.value[id]
    }
  }
)

function navigate(path: string) {
  if (tabs.activeTabId) tabs.navigate(tabs.activeTabId, path)
}

function newTab(path = ROOT) {
  tabs.addTab(path)
}

/** The filter lives in the toolbar, so the pane asking for it takes focus. */
function focusFilter(pane: Pane, recursiveSearch: boolean) {
  tabs.setActivePane(pane.id)
  toolbarRef.value?.focusFilter(recursiveSearch)
}

function toggleFavorite() {
  if (currentPath.value && !readOnlyLocation.value) places.toggleFavorite(currentPath.value)
}

/** Drag the divider; the ratio it writes is what the next start restores. */
function startSplitDrag() {
  const host = contentRef.value
  if (!host) return
  const rect = host.getBoundingClientRect()
  const move = (e: MouseEvent) => view.setSplitRatio((e.clientX - rect.left) / rect.width)
  const stop = () => {
    window.removeEventListener('mousemove', move)
    window.removeEventListener('mouseup', stop)
    document.body.classList.remove('splitting')
  }
  window.addEventListener('mousemove', move)
  window.addEventListener('mouseup', stop)
  document.body.classList.add('splitting')
}

function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)
}

function onKeyDown(e: KeyboardEvent) {
  const typing = isTypingTarget(e.target)
  const mod = e.ctrlKey || e.metaKey
  const id = tabs.activeTabId
  if (!id) return

  // Shortcuts that must work even while the address bar has focus.
  if (mod && e.key.toLowerCase() === 't') {
    e.preventDefault()
    newTab()
    return
  }
  if (mod && e.key.toLowerCase() === 'w') {
    e.preventDefault()
    tabs.closeTab(id)
    return
  }
  if (mod && e.key === '\\') {
    e.preventDefault()
    tabs.toggleSplit()
    return
  }
  // Total Commander's key for "the other panel".
  if (e.key === 'F6') {
    e.preventDefault()
    tabs.focusOtherPane()
    return
  }
  if (mod && e.key.toLowerCase() === 'l') {
    e.preventDefault()
    toolbarRef.value?.focusPathBar()
    return
  }
  // Ctrl+F filters this folder, Ctrl+Shift+F descends — the same query
  // and the same ranking either way.
  if (mod && e.key.toLowerCase() === 'f') {
    e.preventDefault()
    toolbarRef.value?.focusFilter(e.shiftKey)
    return
  }
  if (e.key === 'Escape' && filter.value && !typing) {
    e.preventDefault()
    filter.value = ''
    return
  }
  if (e.altKey && e.key === 'ArrowLeft') {
    e.preventDefault()
    tabs.goBack(id)
    return
  }
  if (e.altKey && e.key === 'ArrowRight') {
    e.preventDefault()
    tabs.goForward(id)
    return
  }
  if (e.altKey && e.key === 'ArrowUp') {
    e.preventDefault()
    if (upTarget.value !== null) navigate(upTarget.value)
    return
  }
  if (e.key === 'F5') {
    e.preventDefault()
    activeBrowser()?.refresh()
    return
  }
  // Row zoom, using the keys browsers already train users on.
  if (mod && (e.key === '=' || e.key === '+')) {
    e.preventDefault()
    view.nudge(1)
    return
  }
  if (mod && e.key === '-') {
    e.preventDefault()
    view.nudge(-1)
    return
  }
  if (mod && e.key === '0') {
    e.preventDefault()
    view.reset()
    return
  }

  if (typing) return

  if (e.key === 'Backspace' && upTarget.value !== null) {
    e.preventDefault()
    navigate(upTarget.value)
  } else if (mod && e.shiftKey && e.key.toLowerCase() === 'n') {
    e.preventDefault()
    activeBrowser()?.newFolder()
  } else if (mod && e.key.toLowerCase() === 'a') {
    e.preventDefault()
    activeBrowser()?.selectAll()
  } else if (mod && e.key.toLowerCase() === 'c') {
    activeBrowser()?.copy()
  } else if (mod && e.key.toLowerCase() === 'x') {
    activeBrowser()?.cut()
  } else if (mod && e.key.toLowerCase() === 'v') {
    activeBrowser()?.paste()
  } else if (e.key === 'F2') {
    e.preventDefault()
    activeBrowser()?.rename()
  } else if (e.key === 'Delete') {
    e.preventDefault()
    activeBrowser()?.remove()
  } else if (mod && e.shiftKey && e.key.toLowerCase() === 'h') {
    e.preventDefault()
    activeBrowser()?.hash()
  }
}

function preventDefaultContextMenu(e: MouseEvent) {
  e.preventDefault()
}

/** Files may have been copied in Explorer while the app was in the background. */
function onWindowFocus() {
  clipboard.refresh()
}

onMounted(async () => {
  await Promise.all([
    tabs.restore().catch((e) => console.error('Cannot restore tabs:', e)),
    view.restore(),
    openWith.restore(),
    archives.restore(),
  ])
  places.refresh().catch((e) => console.error('Cannot load places:', e))
  clipboard.refresh()
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('focus', onWindowFocus)
  document.addEventListener('contextmenu', preventDefaultContextMenu)
  try {
    unlistenOps = await ops.init()
  } catch (e) {
    console.error('Cannot subscribe to file operations:', e)
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('focus', onWindowFocus)
  document.removeEventListener('contextmenu', preventDefaultContextMenu)
  unlistenOps?.()
})
</script>

<template>
  <div class="app">
    <div class="tab-bars" :style="{ gridTemplateColumns: columns }">
      <template v-for="(pane, index) in tabs.panes" :key="pane.id">
        <div v-if="index > 0" class="tab-bars-gap" />
        <TabBar :pane="pane" @new-tab="tabs.addTabIn(pane.id)" />
      </template>
    </div>
    <Toolbar
      ref="toolbarRef"
      :path="currentPath"
      :can-go-back="tabs.canGoBack"
      :can-go-forward="tabs.canGoForward"
      :can-go-up="upTarget !== null"
      :is-favorite="places.isFavorite(currentPath)"
      :can-modify="!readOnlyLocation"
      :filter="filter"
      :recursive="recursive"
      :searching="stats.searching"
      :matches="stats.total"
      :truncated="stats.truncated"
      :split="tabs.split"
      @back="tabs.activeTabId && tabs.goBack(tabs.activeTabId)"
      @forward="tabs.activeTabId && tabs.goForward(tabs.activeTabId)"
      @up="upTarget !== null && navigate(upTarget)"
      @refresh="activeBrowser()?.refresh()"
      @navigate="navigate"
      @new-folder="activeBrowser()?.newFolder()"
      @toggle-favorite="toggleFavorite"
      @toggle-split="tabs.toggleSplit()"
      @settings="settingsOpen = true"
      @update:filter="filter = $event"
      @update:recursive="recursive = $event"
    />

    <main ref="contentRef" class="content" :style="{ gridTemplateColumns: columns }">
      <template v-for="(pane, index) in tabs.panes" :key="pane.id">
        <div
          v-if="index > 0"
          class="splitter"
          title="Drag to resize, double-click to even out"
          @mousedown.prevent="startSplitDrag"
          @dblclick="view.setSplitRatio(0.5)"
        />
        <section
          class="pane"
          :class="{ focused: tabs.split && tabs.activePaneId === pane.id }"
          @mousedown.capture="tabs.setActivePane(pane.id)"
          @focusin="tabs.setActivePane(pane.id)"
        >
          <FileBrowser
            v-if="paneTab(pane)"
            :key="pane.activeTabId"
            :ref="(el) => setBrowser(pane.id, el)"
            :path="paneTab(pane)!.path"
            :filter="paneTab(pane)!.filter"
            :recursive="paneTab(pane)!.recursive"
            @navigate="tabs.navigate(pane.activeTabId, $event)"
            @new-tab="tabs.addTabIn(pane.id, $event)"
            @find="focusFilter(pane, $event)"
            @stats="paneStats[pane.id] = $event"
          />
        </section>
      </template>
    </main>

    <OperationBar />

    <footer class="status">
      <span v-if="filter">
        {{ stats.searching ? 'Searching…' : `${stats.total} match${stats.total === 1 ? '' : 'es'}` }}
        <template v-if="stats.truncated"> (showing the best)</template>
      </span>
      <span v-else>{{ stats.total }} items</span>
      <span v-if="stats.selected">· {{ stats.selected }} selected</span>
      <span class="spacer" />
      <span class="path">{{ currentPath || 'This PC' }}</span>
    </footer>

    <PromptDialog />
    <SettingsDialog v-if="settingsOpen" @close="settingsOpen = false" />
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  overflow: hidden;
}

::-webkit-scrollbar {
  width: 12px;
  height: 12px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: #4f6ec2;
  border-radius: 6px;
}

::-webkit-scrollbar-thumb:hover {
  background: #6b8ae0;
}

::-webkit-scrollbar-thumb:active {
  background: #89b4fa;
}

::-webkit-scrollbar-corner {
  background: transparent;
}

/* The pointer must not become a text cursor halfway through a drag. */
body.splitting {
  cursor: col-resize;
  user-select: none;
}
</style>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: #1e1e2e;
  color: #cdd6f4;
}

.content {
  flex: 1;
  overflow: hidden;
  display: grid;
  min-height: 0;
}

.tab-bars {
  display: grid;
  min-width: 0;
}

.tab-bars-gap {
  background: #181825;
  border-bottom: 1px solid #313244;
  border-left: 1px solid #313244;
}

.pane {
  position: relative;
  overflow: hidden;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

/* Which side a shortcut will act on, stated once and quietly. */
.pane.focused::after {
  content: '';
  position: absolute;
  inset: 0;
  border-top: 2px solid #89b4fa;
  pointer-events: none;
}

.splitter {
  background: #313244;
  cursor: col-resize;
}

.splitter:hover {
  background: #89b4fa;
}

.status {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  padding: 0 10px;
  background: #181825;
  border-top: 1px solid #313244;
  font-size: 11px;
  color: #6c7086;
  flex-shrink: 0;
}

.spacer {
  flex: 1;
}

.path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 60%;
}
</style>
