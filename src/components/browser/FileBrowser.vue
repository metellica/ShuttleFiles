<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { confirm, message, open as openDialog } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import type { FileEntry, HashAlgo } from '@/types/filesystem'
import { ROOT } from '@/types/filesystem'
import * as api from '@/composables/useTauri'
import { promptText } from '@/composables/usePrompt'
import { useFuzzySearch } from '@/composables/useFuzzySearch'
import { matchesName } from '@/composables/useNameMatch'
import { useClipboardStore } from '@/stores/clipboard'
import { useOperationsStore } from '@/stores/operations'
import { usePlacesStore } from '@/stores/places'
import { useOpenWithStore } from '@/stores/openWith'
import { archiveRoot, isInsideArchive, splitArchive, useArchivesStore } from '@/stores/archives'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'
import HashDialog from '@/components/common/HashDialog.vue'
import ArchiveDialog from '@/components/common/ArchiveDialog.vue'
import FastDial from '@/components/browser/FastDial.vue'
import FileList from '@/components/browser/FileList.vue'

const props = defineProps<{ path: string; filter: string; recursive: boolean }>()
const emit = defineEmits<{
  navigate: [path: string]
  'new-tab': [path: string]
  find: [recursive: boolean]
  stats: [value: { total: number; selected: number; searching: boolean; truncated: boolean }]
}>()

const clipboard = useClipboardStore()
const ops = useOperationsStore()
const places = usePlacesStore()
const openWith = useOpenWithStore()
const archives = useArchivesStore()
const search = useFuzzySearch()

const entries = ref<FileEntry[]>([])
const selection = ref<string[]>([])
const loading = ref(false)
const error = ref('')
const listRef = ref<InstanceType<typeof FileList> | null>(null)
const ctx = ref({ visible: false, x: 0, y: 0, items: [] as MenuItem[] })
const hashPaths = ref<string[]>([])
const hashAlgos = ref<HashAlgo[]>(['md5', 'sha256'])
const archiveSources = ref<string[] | null>(null)
const archiveDir = ref('')

/** Guards against an old, slow listing overwriting a newer one. */
let requestId = 0

/** Inside an archive the listing is read-only and served by Rust's reader. */
const insideArchive = computed(() => isInsideArchive(props.path))

// The recursive finder walks the file system, which cannot descend into
// an archive; there, filtering stays a plain match on the level shown.
const searchMode = computed(
  () => props.filter.trim().length > 0 && props.path !== ROOT && !insideArchive.value
)

/**
 * Filtering goes through the same Rust matcher the recursive finder
 * uses, so a query ranks identically whether or not it descends.
 */
const visibleEntries = computed<FileEntry[]>(() => {
  if (searchMode.value) return search.hits.value
  const query = props.filter.trim()
  if (query && insideArchive.value) {
    return entries.value.filter((e) => matchesName(e.name, query))
  }
  return entries.value
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
  [() => props.filter, () => props.recursive, () => props.path],
  ([filter, recursive, path]) =>
    search.schedule(path, insideArchive.value ? '' : filter, recursive),
  { immediate: true }
)

watch(
  [visibleEntries, selection, search.searching, search.truncated],
  ([list, sel, searching, truncated]) =>
    emit('stats', {
      total: searchMode.value ? search.total.value : list.length,
      selected: sel.length,
      searching,
      truncated,
    }),
  { immediate: true }
)

onUnmounted(() => search.dispose())

function open(entry: FileEntry) {
  if (entry.isDir) {
    emit('navigate', entry.path)
    return
  }
  if (insideArchive.value) {
    openArchiveMember(entry)
    return
  }
  // An archive browses like a folder, so a double click steps into it
  // rather than handing it to whatever has the association.
  if (archives.isArchiveFile(entry)) {
    emit('navigate', archiveRoot(entry.path))
    return
  }
  // Double-click always opens with the system default handler.
  reportOpenFailure(api.openEntryPath(entry.path), entry.name)
}

/**
 * Whether the default action hands the file to the configured editor
 * rather than to a folder view or the association. The menu then says
 * "Edit", which is both what happens and what tells the row apart from
 * "Open with System Default" underneath it.
 */
function opensForEditing(entry: FileEntry): boolean {
  if (entry.isDir) return false
  // Not inside one already: an archive browses like a folder.
  if (!insideArchive.value && archives.isArchiveFile(entry)) return false
  return openWith.programFor(entry) !== undefined
}

/**
 * A member has no path on disk, so viewing it means extracting it to a
 * scratch folder first and opening that copy. Edits to it are a copy's
 * edits — the archive stays read-only.
 */
async function openArchiveMember(entry: FileEntry) {
  try {
    const extracted = await api.archiveOpenMember(entry.path)
    reportOpenFailure(openWith.openEntry({ ...entry, path: extracted }), entry.name)
  } catch (e) {
    reportOpenFailure(Promise.reject(e), entry.name)
  }
}

/** Escape hatch for a file the configured editor is the wrong tool for. */
function openWithSystemDefault(entry: FileEntry) {
  reportOpenFailure(api.openEntryPath(entry.path), entry.name)
}

/**
 * VS Code takes the whole selection at once — files as tabs, a folder as
 * the project — which is why it gets a row of its own rather than being
 * one more thing to configure as the text editor.
 */
function openInVscode(paths: string[], name: string) {
  reportOpenFailure(api.openInVscode(paths), name)
}

/**
 * The whole selection when the click landed inside it, the clicked row
 * on its own otherwise — the row the user aimed at is never left out.
 * Copied out of the reactive array so what crosses the IPC is plain.
 */
function selectionOr(entry: FileEntry): string[] {
  return selection.value.includes(entry.path) ? [...selection.value] : [entry.path]
}

/**
 * A failed open used to only reach the console, which made a misconfigured
 * editor or a missing association look like a dead menu entry.
 */
function reportOpenFailure(attempt: Promise<void>, name: string) {
  attempt.catch((e) => {
    console.error('Cannot open file:', e)
    message(String(e), { title: `Cannot open ${name}`, kind: 'error' }).catch(() => {})
  })
}

function selectedEntries(): FileEntry[] {
  return entries.value.filter((e) => selection.value.includes(e.path))
}

async function copyPathsToOsClipboard() {
  await navigator.clipboard.writeText(selection.value.join('\r\n'))
}

async function doCopy() {
  if (insideArchive.value) return
  await clipboard.copy(selection.value)
}

async function doCut() {
  if (insideArchive.value) return
  await clipboard.cut(selection.value)
}

/** Reads the system clipboard, so files copied in Explorer paste here too. */
async function doPaste() {
  if (!props.path || insideArchive.value) return
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
  if (targets.length === 0 || insideArchive.value) return
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
  if (!entry || insideArchive.value) return
  const name = await promptText('Rename to:', entry.name)
  if (!name || name === entry.name) return
  const parent = await api.parentPath(entry.path)
  if (!parent && parent !== '') return
  try {
    await api.renameEntry(entry.path, joinPath(props.path, name))
    await load()
  } catch (e) {
    error.value = String(e)
  }
}

async function newFolder() {
  if (!props.path || insideArchive.value) return
  const name = await promptText('New folder name:', 'New folder')
  if (!name) return
  try {
    await api.createDir(joinPath(props.path, name))
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

/** Checksums are only meaningful for files, so folders are never included. */
function selectedFiles(): string[] {
  const known = new Map(visibleEntries.value.map((e) => [e.path, e]))
  return selection.value.filter((p) => !known.get(p)?.isDir)
}

function openHashDialog(algos: HashAlgo[] = ['md5', 'sha256']) {
  const files = selectedFiles()
  if (files.length === 0) return
  hashAlgos.value = algos
  hashPaths.value = files
}

/** The checksum entry, as a submenu so a single digest can be picked. */
function hashMenu(): MenuItem {
  const count = selectedFiles().length
  return {
    label: count > 1 ? `Checksum (${count})` : 'Checksum',
    icon: '#️⃣',
    disabled: count === 0,
    children: [
      { label: 'MD5', action: () => openHashDialog(['md5']) },
      { label: 'SHA-256', action: () => openHashDialog(['sha256']) },
      { separator: true },
      { label: 'MD5 and SHA-256', action: () => openHashDialog(['md5', 'sha256']) },
    ],
  }
}

/**
 * A search hit is usually somewhere the current tab is not, so the useful
 * "new tab" target is the folder it lives in; for a folder, itself.
 */
async function openInNewTab(entry: FileEntry) {
  if (entry.isDir) {
    emit('new-tab', entry.path)
    return
  }
  try {
    const parent = await api.parentPath(entry.path)
    if (parent) emit('new-tab', parent)
  } catch (e) {
    error.value = String(e)
  }
}

/** Join a folder and a child name with whichever separator is in use. */
function joinPath(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`
}

/**
 * Where "here" is for an extraction: the folder on screen, or — when
 * the browser is inside an archive — the folder that holds it, since an
 * archive cannot be written into.
 */
async function extractionBase(): Promise<string> {
  if (!insideArchive.value) return props.path
  const split = splitArchive(props.path)
  if (!split) return props.path
  return (await api.parentPath(split.archive)) ?? props.path
}

async function extractTo(sources: string[], dest: string) {
  if (sources.length === 0 || !dest) return
  try {
    await ops.start('extract', sources, dest)
  } catch (e) {
    error.value = String(e)
  }
}

async function extractHere(sources: string[]) {
  await extractTo(sources, await extractionBase())
}

/** "Extract to name\" — the archive's own name, so nothing spills out. */
async function extractToNamedFolder(sources: string[], folder: string) {
  await extractTo(sources, joinPath(await extractionBase(), folder))
}

async function extractToChosenFolder(sources: string[]) {
  const picked = await openDialog({
    title: 'Extract to',
    directory: true,
    multiple: false,
    defaultPath: await extractionBase(),
  })
  if (typeof picked === 'string') await extractTo(sources, picked)
}

/** Archive name without its extension, for the "extract to" folder. */
function archiveStem(name: string): string {
  const lower = name.toLowerCase()
  const ext = archives.extensions.find((e) => lower.endsWith(`.${e}`))
  return ext ? name.slice(0, name.length - ext.length - 1) : name
}

/** One-click zip of the selection, next to it. */
async function quickCompress(sources: string[]) {
  if (sources.length === 0) return
  try {
    const name = await api.archiveSuggestName(props.path, sources, 'zip')
    await ops.start('compress', sources, props.path, {
      archivePath: joinPath(props.path, name),
      level: 6,
    })
  } catch (e) {
    error.value = String(e)
  }
}

/** The archive dialog needs the folder the new archive lands in. */
function openArchiveDialog(sources: string[], dir?: string) {
  archiveDir.value = dir ?? props.path
  archiveSources.value = sources
}

/** Extract / compress entries for the clicked row. */
function archiveMenu(entry: FileEntry): MenuItem[] {  const targets = selection.value.includes(entry.path) ? [...selection.value] : [entry.path]

  if (insideArchive.value) {
    return [
      { separator: true },
      { label: 'Extract Here', icon: '📤', action: () => extractHere(targets) },
      { label: 'Extract To…', icon: '📂', action: () => extractToChosenFolder(targets) },
    ]
  }

  const items: MenuItem[] = [{ separator: true }]
  if (archives.isArchiveFile(entry)) {
    items.push(
      { label: 'Extract Here', icon: '📤', action: () => extractHere([entry.path]) },
      {
        label: `Extract to "${archiveStem(entry.name)}\\"`,
        icon: '📁',
        action: () => extractToNamedFolder([entry.path], archiveStem(entry.name)),
      },
      { label: 'Extract To…', icon: '📂', action: () => extractToChosenFolder([entry.path]) }
    )
  }
  items.push(
    {
      label: targets.length > 1 ? `Add ${targets.length} Items to Zip` : 'Add to Zip',
      icon: '🗜',
      action: () => quickCompress(targets),
    },
    {
      label: 'Add to Archive…',
      icon: '📦',
      action: () => openArchiveDialog(targets),
    }
  )
  return items
}

async function openContextMenu(event: MouseEvent, entry: FileEntry | null) {
  // The clipboard may have been filled by Explorer since the last check.
  await clipboard.refresh()
  // Anchor the native menu where the click happened, not where the
  // pointer ends up after travelling to the "More options" row.
  const anchor = { x: event.screenX, y: event.screenY }
  const many = selection.value.length
  // Members live inside the archive file, so nothing that writes to the
  // file system in place is offered for them.
  const readOnly = insideArchive.value
  // Asked on each menu rather than once at startup: the backend caches
  // the answer, so this costs a message and not a search.
  const hasVscode = await api.vscodeAvailable()
  const editing = !!entry && opensForEditing(entry)
  // Snapshot at right-click time, so the label and what gets opened
  // agree even if the list reloads while the menu is up.
  const targets = entry ? selectionOr(entry) : []
  const items: MenuItem[] = entry
    ? [
        {
          label: editing ? 'Edit' : 'Open',
          // Not the pencil: Rename further down already wears it.
          icon: editing ? '📝' : '↩',
          action: () => open(entry),
        },
        ...(openWith.programFor(entry) || (!readOnly && archives.isArchiveFile(entry))
          ? [
              {
                label: 'Open with System Default',
                icon: '🖥',
                action: () => openWithSystemDefault(entry),
              },
            ]
          : []),
        ...(hasVscode && !readOnly
          ? [
              {
                label:
                  targets.length > 1
                    ? `Open with VS Code (${targets.length})`
                    : 'Open with VS Code',
                icon: '💻',
                action: () => openInVscode(targets, entry.name),
              },
            ]
          : []),
        ...(entry.isDir || searchMode.value
          ? [
              {
                label: entry.isDir ? 'Open in New Tab' : 'Open Containing Folder in New Tab',
                icon: '⧉',
                action: () => openInNewTab(entry),
              },
            ]
          : []),
        ...archiveMenu(entry),
        ...(readOnly
          ? []
          : [
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
            ]),
        { separator: true },
        ...(entry.isDir && !readOnly
          ? [
              {
                label: 'Find in This Folder…',
                icon: '🔍',
                action: () => {
                  emit('navigate', entry.path)
                  emit('find', true)
                },
              },
            ]
          : []),
        ...(!entry.isDir && !readOnly ? [hashMenu()] : []),
        ...(entry.isDir && !readOnly
          ? [
              {
                label: places.isFavorite(entry.path) ? 'Remove Favorite' : 'Add to Favorites',
                icon: '⭐',
                action: () => places.toggleFavorite(entry.path, entry.name),
              },
            ]
          : []),
        { label: 'Copy Path', icon: '🔗', action: copyPathsToOsClipboard },
        ...(readOnly
          ? []
          : [
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
            ]),
      ]
    : readOnly
      ? [
          {
            label: splitArchive(props.path)?.inner ? 'Extract This Folder Here' : 'Extract All Here',
            icon: '📤',
            action: () => extractHere([props.path]),
          },
          {
            label: 'Extract To…',
            icon: '📂',
            action: () => extractToChosenFolder([props.path]),
          },
          { separator: true },
          { label: 'Refresh', icon: '⟳', action: load },
          {
            label: 'Filter This Folder… (Ctrl+F)',
            icon: '🔤',
            action: () => emit('find', false),
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
          label: 'Find in Folder… (Ctrl+Shift+F)',
          icon: '🔍',
          action: () => emit('find', true),
        },
        {
          label: 'Filter This Folder… (Ctrl+F)',
          icon: '🔤',
          action: () => emit('find', false),
        },
        { separator: true },
        {
          label: places.isFavorite(props.path) ? 'Remove Favorite' : 'Add to Favorites',
          icon: '⭐',
          action: () => places.toggleFavorite(props.path),
        },
        {
          label: 'Open in Explorer',
          icon: '🗂',
          action: () => api.openEntryPath(props.path).catch(console.error),
        },
        ...(hasVscode && props.path !== ROOT
          ? [
              {
                label: 'Open Folder with VS Code',
                icon: '💻',
                action: () => openInVscode([props.path], props.path),
              },
            ]
          : []),
        { separator: true },
        {
          label: 'Add Folder to Archive…',
          icon: '📦',
          action: async () =>
            openArchiveDialog([props.path], (await api.parentPath(props.path)) ?? props.path),
        },
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
  hash: () => openHashDialog(),
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
      :error="error || search.error.value"
      :search-mode="searchMode"
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
    <HashDialog
      v-if="hashPaths.length"
      :paths="hashPaths"
      :algos="hashAlgos"
      @close="hashPaths = []"
    />
    <ArchiveDialog
      v-if="archiveSources"
      :sources="archiveSources"
      :dir="archiveDir"
      @close="archiveSources = null"
    />
  </div>
</template>

<style scoped>
.browser {
  height: 100%;
  overflow: hidden;
}
</style>
