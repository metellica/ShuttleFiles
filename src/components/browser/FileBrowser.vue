<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { confirm } from '@tauri-apps/plugin-dialog'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import type { FileEntry } from '@/types/filesystem'
import { ROOT } from '@/types/filesystem'
import * as api from '@/composables/useTauri'
import { promptText } from '@/composables/usePrompt'
import { useClipboardStore } from '@/stores/clipboard'
import { useOperationsStore } from '@/stores/operations'
import { usePlacesStore } from '@/stores/places'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'
import FastDial from '@/components/browser/FastDial.vue'
import FileList from '@/components/browser/FileList.vue'

const props = defineProps<{ path: string; filter: string }>()
const emit = defineEmits<{
  navigate: [path: string]
  'new-tab': [path: string]
  stats: [value: { total: number; selected: number }]
}>()

const clipboard = useClipboardStore()
const ops = useOperationsStore()
const places = usePlacesStore()

const entries = ref<FileEntry[]>([])
const selection = ref<string[]>([])
const loading = ref(false)
const error = ref('')
const listRef = ref<InstanceType<typeof FileList> | null>(null)
const ctx = ref({ visible: false, x: 0, y: 0, items: [] as MenuItem[] })

/** Guards against an old, slow listing overwriting a newer one. */
let requestId = 0

const visibleEntries = computed(() => {
  const needle = props.filter.trim().toLowerCase()
  if (!needle) return entries.value
  return entries.value.filter((e) => e.name.toLowerCase().includes(needle))
})

async function load() {
  if (props.path === ROOT) {
    entries.value = []
    error.value = ''
    return
  }
  const id = ++requestId
  loading.value = true
  error.value = ''
  try {
    const listing = await api.listDir(props.path)
    if (id !== requestId) return
    entries.value = listing.entries
  } catch (e) {
    if (id !== requestId) return
    entries.value = []
    error.value = String(e)
  } finally {
    if (id === requestId) loading.value = false
  }
}

watch(() => props.path, load, { immediate: true })

// A finished copy/move/delete may have changed this folder; reload
// without the browser needing to know which job touched what.
watch(() => ops.completionTick, load)

watch(
  [visibleEntries, selection],
  ([list, sel]) => emit('stats', { total: list.length, selected: sel.length }),
  { immediate: true }
)

function open(entry: FileEntry) {
  if (entry.isDir) {
    emit('navigate', entry.path)
  } else {
    openPath(entry.path).catch((e) => console.error('Cannot open file:', e))
  }
}

function selectedEntries(): FileEntry[] {
  return entries.value.filter((e) => selection.value.includes(e.path))
}

async function copyPathsToOsClipboard() {
  await navigator.clipboard.writeText(selection.value.join('\r\n'))
}

async function doCopy() {
  await clipboard.copy(selection.value)
}

async function doCut() {
  await clipboard.cut(selection.value)
}

/** Reads the system clipboard, so files copied in Explorer paste here too. */
async function doPaste() {
  if (!props.path) return
  try {
    const { paths, cut } = await clipboard.read()
    if (paths.length === 0) return
    await ops.start(cut ? 'move' : 'copy', paths, props.path)
  } catch (e) {
    error.value = String(e)
  }
}

async function doDelete() {
  const targets = [...selection.value]
  if (targets.length === 0) return
  const label = targets.length === 1 ? targets[0] : `${targets.length} selected items`
  const ok = await confirm(`Permanently delete ${label}?`, {
    title: 'Delete',
    kind: 'warning',
  })
  if (!ok) return
  try {
    await ops.start('delete', targets)
  } catch (e) {
    error.value = String(e)
  }
}

async function doRename() {
  const entry = selectedEntries()[0]
  if (!entry) return
  const name = await promptText('Rename to:', entry.name)
  if (!name || name === entry.name) return
  const parent = await api.parentPath(entry.path)
  if (!parent && parent !== '') return
  const sep = props.path.includes('\\') ? '\\' : '/'
  const target = props.path.endsWith(sep)
    ? `${props.path}${name}`
    : `${props.path}${sep}${name}`
  try {
    await api.renameEntry(entry.path, target)
    await load()
  } catch (e) {
    error.value = String(e)
  }
}

async function newFolder() {
  if (!props.path) return
  const name = await promptText('New folder name:', 'New folder')
  if (!name) return
  const sep = props.path.includes('\\') ? '\\' : '/'
  const target = props.path.endsWith(sep)
    ? `${props.path}${name}`
    : `${props.path}${sep}${name}`
  try {
    await api.createDir(target)
    await load()
  } catch (e) {
    error.value = String(e)
  }
}

/**
 * Hand off to the native shell menu, which is where third-party
 * extensions (7-Zip, TortoiseGit, WinMerge, …) live. It is hosted by a
 * separate process, so a misbehaving extension cannot take this window
 * down with it.
 */
async function showShellMenu(paths: string[], anchor: { x: number; y: number }) {
  if (paths.length === 0) return
  try {
    const result = await api.shellMenuShow(paths, anchor.x, anchor.y)
    // Almost any verb can change the folder (extract here, delete,
    // rename…), so refresh whenever something actually ran.
    if (result.invoked) await load()
  } catch (e) {
    error.value = String(e)
  }
}

async function openContextMenu(event: MouseEvent, entry: FileEntry | null) {
  // The clipboard may have been filled by Explorer since the last check.
  await clipboard.refresh()
  // Anchor the native menu where the click happened, not where the
  // pointer ends up after travelling to the "More options" row.
  const anchor = { x: event.screenX, y: event.screenY }
  const many = selection.value.length
  const items: MenuItem[] = entry
    ? [
        { label: 'Open', icon: '↩', action: () => open(entry) },
        ...(entry.isDir
          ? [
              {
                label: 'Open in New Tab',
                icon: '⧉',
                action: () => emit('new-tab', entry.path),
              },
            ]
          : []),
        { separator: true },
        {
          label: many > 1 ? `Copy (${many})` : 'Copy',
          icon: '📋',
          action: doCopy,
        },
        {
          label: many > 1 ? `Cut (${many})` : 'Cut',
          icon: '✂',
          action: doCut,
        },
        {
          label: 'Paste',
          icon: '📥',
          disabled: !clipboard.hasContent,
          action: doPaste,
        },
        { separator: true },
        { label: 'Rename', icon: '✏️', disabled: many !== 1, action: doRename },
        { label: 'Delete', icon: '🗑', danger: true, action: doDelete },
        { separator: true },
        ...(entry.isDir
          ? [
              {
                label: places.isFavorite(entry.path) ? 'Remove Favorite' : 'Add to Favorites',
                icon: '⭐',
                action: () => places.toggleFavorite(entry.path, entry.name),
              },
            ]
          : []),
        { label: 'Copy Path', icon: '🔗', action: copyPathsToOsClipboard },
        {
          label: 'Show in Explorer',
          icon: '🗂',
          action: () => revealItemInDir(entry.path).catch(console.error),
        },
        { separator: true },
        {
          label: 'More options',
          icon: '⋯',
          action: () => showShellMenu(selection.value, anchor),
        },
      ]
    : [
        {
          label: 'Paste',
          icon: '📥',
          disabled: !clipboard.hasContent,
          action: doPaste,
        },
        { label: 'New Folder', icon: '📁', action: newFolder },
        { label: 'Refresh', icon: '⟳', action: load },
        { separator: true },
        {
          label: places.isFavorite(props.path) ? 'Remove Favorite' : 'Add to Favorites',
          icon: '⭐',
          action: () => places.toggleFavorite(props.path),
        },
        {
          label: 'Open in Explorer',
          icon: '🗂',
          action: () => openPath(props.path).catch(console.error),
        },
        { separator: true },
        {
          label: 'More options',
          icon: '⋯',
          action: () => showShellMenu([props.path], anchor),
        },
      ]

  ctx.value = { visible: true, x: event.clientX, y: event.clientY, items }
}

defineExpose({
  refresh: load,
  newFolder,
  rename: doRename,
  remove: doDelete,
  paste: doPaste,
  copy: doCopy,
  cut: doCut,
  selectAll: () => listRef.value?.selectAll(),
})
</script>

<template>
  <div class="browser">
    <FastDial v-if="props.path === ROOT" @navigate="emit('navigate', $event)" />
    <FileList
      v-else
      ref="listRef"
      :entries="visibleEntries"
      :loading="loading"
      :error="error"
      @open="open"
      @context="openContextMenu"
      @selection-change="selection = $event"
    />
    <ContextMenu
      :visible="ctx.visible"
      :x="ctx.x"
      :y="ctx.y"
      :items="ctx.items"
      @close="ctx.visible = false"
    />
  </div>
</template>

<style scoped>
.browser {
  height: 100%;
  overflow: hidden;
}
</style>
