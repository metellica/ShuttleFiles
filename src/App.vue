<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { useTabsStore } from '@/stores/tabs'
import { usePlacesStore } from '@/stores/places'
import { useClipboardStore } from '@/stores/clipboard'
import { useOperationsStore } from '@/stores/operations'
import { useViewSettingsStore } from '@/stores/viewSettings'
import { useOpenWithStore } from '@/stores/openWith'
import { parentPath } from '@/composables/useTauri'
import { ROOT } from '@/types/filesystem'
import TabBar from '@/components/layout/TabBar.vue'
import Toolbar from '@/components/layout/Toolbar.vue'
import OperationBar from '@/components/layout/OperationBar.vue'
import FileBrowser from '@/components/browser/FileBrowser.vue'
import PromptDialog from '@/components/common/PromptDialog.vue'
import SettingsDialog from '@/components/common/SettingsDialog.vue'

const tabs = useTabsStore()
const places = usePlacesStore()
const clipboard = useClipboardStore()
const ops = useOperationsStore()
const view = useViewSettingsStore()
const openWith = useOpenWithStore()

let unlistenOps: UnlistenFn | null = null

const stats = ref({ total: 0, selected: 0, searching: false, truncated: false })
const upTarget = ref<string | null>(null)
const browserRef = ref<InstanceType<typeof FileBrowser> | null>(null)
const toolbarRef = ref<InstanceType<typeof Toolbar> | null>(null)
const settingsOpen = ref(false)

const currentPath = computed(() => tabs.activeTab?.path ?? ROOT)

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

function navigate(path: string) {
  if (tabs.activeTabId) tabs.navigate(tabs.activeTabId, path)
}

function newTab(path = ROOT) {
  tabs.addTab(path)
}

function toggleFavorite() {
  if (currentPath.value) places.toggleFavorite(currentPath.value)
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
    browserRef.value?.refresh()
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
    browserRef.value?.newFolder()
  } else if (mod && e.key.toLowerCase() === 'a') {
    e.preventDefault()
    browserRef.value?.selectAll()
  } else if (mod && e.key.toLowerCase() === 'c') {
    browserRef.value?.copy()
  } else if (mod && e.key.toLowerCase() === 'x') {
    browserRef.value?.cut()
  } else if (mod && e.key.toLowerCase() === 'v') {
    browserRef.value?.paste()
  } else if (e.key === 'F2') {
    e.preventDefault()
    browserRef.value?.rename()
  } else if (e.key === 'Delete') {
    e.preventDefault()
    browserRef.value?.remove()
  } else if (mod && e.shiftKey && e.key.toLowerCase() === 'h') {
    e.preventDefault()
    browserRef.value?.hash()
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
    <TabBar @new-tab="newTab()" />
    <Toolbar
      ref="toolbarRef"
      :path="currentPath"
      :can-go-back="tabs.canGoBack"
      :can-go-forward="tabs.canGoForward"
      :can-go-up="upTarget !== null"
      :is-favorite="places.isFavorite(currentPath)"
      :filter="filter"
      :recursive="recursive"
      :searching="stats.searching"
      :matches="stats.total"
      :truncated="stats.truncated"
      @back="tabs.activeTabId && tabs.goBack(tabs.activeTabId)"
      @forward="tabs.activeTabId && tabs.goForward(tabs.activeTabId)"
      @up="upTarget !== null && navigate(upTarget)"
      @refresh="browserRef?.refresh()"
      @navigate="navigate"
      @new-folder="browserRef?.newFolder()"
      @toggle-favorite="toggleFavorite"
      @settings="settingsOpen = true"
      @update:filter="filter = $event"
      @update:recursive="recursive = $event"
    />

    <main class="content">
      <FileBrowser
        v-if="tabs.activeTab"
        :key="tabs.activeTab.id"
        ref="browserRef"
        :path="currentPath"
        :filter="filter"
        :recursive="recursive"
        @navigate="navigate"
        @new-tab="newTab($event)"
        @find="toolbarRef?.focusFilter($event)"
        @stats="stats = $event"
      />
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
