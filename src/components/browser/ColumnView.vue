<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import type { FileEntry } from '@/types/filesystem'
import { fileIcon, formatSize } from '@/composables/useFormat'
import { useViewSettingsStore } from '@/stores/viewSettings'
import * as api from '@/composables/useTauri'

/**
 * Finder-style Miller columns: one column per ancestor of the current
 * path, the last one listing the folder itself. Clicking a directory
 * opens it in the next column; a file is selected in place. The parent
 * of every level is always visible, so there is no ".." row here.
 */

interface Column {
  /** Directory this column lists. */
  path: string
  entries: FileEntry[]
  loading: boolean
  error: string
  /** Path of the entry opened inside this column (drives the next column). */
  selectedPath: string | null
}

const props = defineProps<{ path: string }>()

const emit = defineEmits<{
  /** A file was double-clicked (dirs navigate on a single click instead). */
  open: [entry: FileEntry]
  /** The folder the tab should browse now, after a click or a hover-open. */
  navigate: [path: string]
  /** `entry` is null for a blank-area menu, which targets that column's `dir`. */
  context: [event: MouseEvent, entry: FileEntry | null, dir: string]
  'selection-change': [paths: string[]]
  'entries-change': [entries: FileEntry[]]
  move: [sources: string[], destDir: string]
  stats: [value: { total: number; selected: number; searching: boolean; truncated: boolean }]
}>()

const view = useViewSettingsStore()
const columns = ref<Column[]>([])
const selected = ref<Set<string>>(new Set())
/** Anchor for shift+click range selection, scoped to one column. */
const anchor = ref<{ colIndex: number; path: string } | null>(null)
const rootRef = ref<HTMLElement | null>(null)
const columnsEl = ref<HTMLElement | null>(null)

/** Guards against an old, slow rebuild landing after a newer one. */
let requestId = 0

function sortEntries(entries: FileEntry[]): FileEntry[] {
  return [...entries].sort((a, b) => {
    // Directories lead, whatever the alphabet says.
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
    return a.name.localeCompare(b.name, undefined, { numeric: true })
  })
}

async function loadColumn(path: string): Promise<Column> {
  const col: Column = { path, entries: [], loading: true, error: '', selectedPath: null }
  try {
    const listing = await api.listDir(path)
    col.entries = sortEntries(listing.entries)
  } catch (e) {
    col.error = String(e)
  } finally {
    col.loading = false
  }
  return col
}

const allEntries = computed(() => columns.value.flatMap((c) => c.entries))

function commitSelection() {
  emit('selection-change', [...selected.value].filter((p) => entriesAt(p)))
}

/** Whether some loaded column actually lists `path`. */
function entriesAt(path: string): boolean {
  return allEntries.value.some((e) => e.path === path)
}

watch([allEntries, selected], () => {
  emit('entries-change', allEntries.value)
  emit('stats', {
    total: allEntries.value.length,
    selected: selected.value.size,
    searching: false,
    truncated: false,
  })
})

/**
 * One column per ancestor of `path`, outermost first, each pointing at
 * the next through `selectedPath`. The chain comes from the backend, so
 * drive roots, UNC shares and `archive.zip!\inner` paths all resolve the
 * same way.
 */
async function rebuild(path: string) {
  const id = ++requestId
  selected.value = new Set()
  anchor.value = null

  let chain: string[] = [path]
  try {
    chain = (await api.breadcrumbs(path)).map(([, p]) => p)
  } catch (e) {
    console.error('Cannot resolve the folder chain:', e)
  }
  if (id !== requestId) return

  const loaded = await Promise.all(chain.map((dir) => loadColumn(dir)))
  if (id !== requestId) return

  // A path pointing at a file (typed or pasted into the address bar) has
  // no listing of its own: select it in its parent column rather than
  // showing a broken empty one.
  if (loaded.length >= 2) {
    const last = loaded[loaded.length - 1]!
    const parent = loaded[loaded.length - 2]!
    const filePath = chain[chain.length - 1]!
    const fileEntry = last.error
      ? parent.entries.find((e) => e.path === filePath && !e.isDir) ?? null
      : null
    if (fileEntry) {
      loaded.pop()
      chain.pop()
      parent.selectedPath = fileEntry.path
      selected.value = new Set([fileEntry.path])
      anchor.value = { colIndex: loaded.length - 1, path: fileEntry.path }
    }
  }

  for (let i = 0; i < loaded.length - 1; i++) {
    loaded[i]!.selectedPath = chain[i + 1]!
  }
  columns.value = loaded
  commitSelection()
  scrollToEnd()
  revealOpened()
}

/**
 * React to the tab moving somewhere else. A click in this view has
 * usually already put the columns where they belong, so the cheap cases
 * are recognised and left alone:
 * - the last column already lists `path` (a file click in its own column);
 * - the last column has `path` selected and `path` is its direct child
 *   (a directory click) — only the child column still has to load.
 */
async function setPath(path: string) {
  const last = columns.value[columns.value.length - 1]
  if (last) {
    if (last.path === path) return
    if (last.selectedPath === path && isDirectChildOf(last.path, path)) {
      const id = ++requestId
      const col = await loadColumn(path)
      if (id !== requestId) return
      columns.value = [...columns.value, col]
      scrollToEnd()
      return
    }
  }
  await rebuild(path)
}

onMounted(() => setPath(props.path))

/** True when `child` sits directly inside `parent`. */
function isDirectChildOf(parent: string, child: string): boolean {
  const np = parent.replace(/[\\/]+$/, '').replace(/\//g, '\\')
  const nc = child.replace(/[\\/]+$/, '').replace(/\//g, '\\')
  const sep = nc.lastIndexOf('\\')
  return sep > 0 && nc.substring(0, sep) === np
}

function scrollToEnd() {
  requestAnimationFrame(() => {
    columnsEl.value?.scrollTo({ left: columnsEl.value.scrollWidth, behavior: 'smooth' })
  })
}

/** Opened entries may sit below the fold of tall columns; nudge them into view. */
function revealOpened() {
  nextTick(() => {
    for (const el of columnsEl.value?.querySelectorAll('.entry.opened') ?? []) {
      ;(el as HTMLElement).scrollIntoView({ block: 'nearest' })
    }
  })
}

function selectRange(entries: FileEntry[], entry: FileEntry, anchorPath: string | null, additive: boolean) {
  const clickedIdx = entries.findIndex((e) => e.path === entry.path)
  let anchorIdx = anchorPath ? entries.findIndex((e) => e.path === anchorPath) : -1
  if (anchorIdx === -1) anchorIdx = clickedIdx
  if (!additive) selected.value = new Set()
  const lo = Math.min(anchorIdx, clickedIdx)
  const hi = Math.max(anchorIdx, clickedIdx)
  for (let i = lo; i <= hi; i++) {
    const e = entries[i]
    if (e) selected.value.add(e.path)
  }
}

function onEntryClick(colIndex: number, entry: FileEntry, event: MouseEvent) {
  const col = columns.value[colIndex]
  if (!col) return

  if (event.shiftKey) {
    // Range select within this column, anchored at the last plain click.
    const anchorPath = anchor.value && anchor.value.colIndex === colIndex ? anchor.value.path : null
    selectRange(col.entries, entry, anchorPath, event.ctrlKey || event.metaKey)
    anchor.value = { colIndex, path: entry.path }
    commitSelection()
    return
  }

  if (event.ctrlKey || event.metaKey) {
    // Multi-select across columns without disturbing the layout.
    const next = new Set(selected.value)
    if (next.has(entry.path)) next.delete(entry.path)
    else {
      next.add(entry.path)
      anchor.value = { colIndex, path: entry.path }
    }
    selected.value = next
    commitSelection()
    return
  }

  selected.value = new Set([entry.path])
  anchor.value = { colIndex, path: entry.path }
  col.selectedPath = entry.path

  // Whatever sits beyond this column is no longer reachable on screen.
  columns.value = columns.value.slice(0, colIndex + 1)

  if (entry.isDir) {
    loadInto(entry.path)
    emit('navigate', entry.path)
  } else if (col.path !== props.path) {
    // Selecting a file in an earlier column makes that column the current one.
    emit('navigate', col.path)
  }
  commitSelection()
}

/** Load `path` as the new last column, discarding the load if the user has clicked elsewhere meanwhile. */
async function loadInto(path: string) {
  const id = ++requestId
  const col = await loadColumn(path)
  if (id !== requestId) return
  columns.value = [...columns.value, col]
  scrollToEnd()
}

function onEntryDblClick(entry: FileEntry) {
  // A directory was already entered by the first click of the pair.
  if (!entry.isDir) emit('open', entry)
}

/** Left-click on blank space clears the selection. */
function onBlankClick() {
  selected.value = new Set()
  anchor.value = null
  commitSelection()
}

function onEntryContext(colIndex: number, entry: FileEntry, event: MouseEvent) {
  // Right-clicking outside the selection selects the row first, as Explorer does.
  if (!selected.value.has(entry.path)) {
    selected.value = new Set([entry.path])
    anchor.value = { colIndex, path: entry.path }
    commitSelection()
  }
  emit('context', event, entry, columns.value[colIndex]?.path ?? props.path)
}

/** Right-click on the blank area of a column: a dir-level menu for that column. */
function onColContext(col: Column, event: MouseEvent) {
  emit('context', event, null, col.path)
}

function onRootContext(event: MouseEvent) {
  emit('context', event, null, props.path)
}

// --- Drag & drop (move) -----------------------------------------------------

const DND_MIME = 'application/x-shuttle-files-paths'

function isFileDrag(event: DragEvent): boolean {
  const types = event.dataTransfer?.types
  return !!types && Array.from(types).includes(DND_MIME)
}

function parseDragPaths(event: DragEvent): string[] {
  const raw = event.dataTransfer?.getData(DND_MIME)
  if (!raw) return []
  try {
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === 'string') : []
  } catch {
    return []
  }
}

function normalizePath(path: string): string {
  return path.replace(/[\\/]+$/, '').replace(/\//g, '\\').toLowerCase()
}

/** Containing folder of a normalized path, or '' if it has no separator. */
function parentOf(normalizedPath: string): string {
  const idx = normalizedPath.lastIndexOf('\\')
  return idx === -1 ? '' : normalizedPath.slice(0, idx)
}

/**
 * Moving into itself, into a descendant, or right back into the folder
 * it's already sitting in are all no-ops the backend would otherwise turn
 * into something surprising: a same-folder drop collides with the source
 * itself, so the paste logic's collision-avoidance renames the file
 * (`report.txt` -> `report (2).txt`) instead of doing nothing.
 */
function canMoveTo(sources: string[], destDir: string): boolean {
  const dest = normalizePath(destDir)
  return sources.length > 0 && sources.every((src) => {
    const item = normalizePath(src)
    return item !== dest && !dest.startsWith(`${item}\\`) && parentOf(item) !== dest
  })
}

/** How long a drag has to dwell on a folder before it springs open. */
const HOVER_OPEN_DELAY = 700
let hoverTimer: ReturnType<typeof setTimeout> | null = null
const hoverDirPath = ref<string | null>(null)

/**
 * Stepping into a folder mid-drag reaches destinations several levels
 * deeper without a separate trip. Marking the folder opened first lets
 * setPath take the cheap "append one column" branch instead of
 * rebuilding every ancestor in the middle of the drag.
 */
function armHoverOpen(colIndex: number, entry: FileEntry) {
  if (hoverDirPath.value === entry.path) return
  clearHoverOpen()
  hoverDirPath.value = entry.path
  hoverTimer = setTimeout(() => {
    hoverTimer = null
    hoverDirPath.value = null
    const col = columns.value[colIndex]
    if (col) col.selectedPath = entry.path
    emit('open', entry)
  }, HOVER_OPEN_DELAY)
}

function clearHoverOpen() {
  if (hoverTimer !== null) {
    clearTimeout(hoverTimer)
    hoverTimer = null
  }
  hoverDirPath.value = null
}

onBeforeUnmount(clearHoverOpen)

function onRowDragStart(entry: FileEntry, colIndex: number, event: DragEvent) {
  if (!selected.value.has(entry.path)) {
    selected.value = new Set([entry.path])
    anchor.value = { colIndex, path: entry.path }
    commitSelection()
  }
  // Only entries still listed somewhere may travel.
  const sources = [...selected.value].filter((p) => entriesAt(p))
  if (!event.dataTransfer || sources.length === 0) return
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData(DND_MIME, JSON.stringify(sources))
  event.dataTransfer.setData('text/plain', sources.join('\n'))
}

function onRowDragEnter(entry: FileEntry, colIndex: number, event: DragEvent) {
  if (!entry.isDir || !isFileDrag(event)) return
  event.preventDefault()
  // `dragover` bubbles continuously while the pointer sits over the row;
  // left unstopped, every tick would also reach the column and root
  // handlers and clear the hover-open timer this row just armed.
  event.stopPropagation()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  dropTargetDir.value = entry.path
  armHoverOpen(colIndex, entry)
}

function onRowDragOver(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir || !isFileDrag(event)) return
  event.preventDefault()
  event.stopPropagation()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}

function onRowDrop(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir) return
  const sources = parseDragPaths(event)
  event.preventDefault()
  // A drop is handled exactly once — the column and root fallbacks would
  // otherwise move the same files a second time, into a different folder.
  event.stopPropagation()
  dropTargetDir.value = null
  dropColIndex.value = null
  clearHoverOpen()
  if (canMoveTo(sources, entry.path)) emit('move', sources, entry.path)
}

/**
 * `dragleave` fires on every child element, not just the row boundary.
 * Checking `relatedTarget` keeps the drop highlight and hover timer alive
 * while the pointer is still somewhere inside the row; the path check
 * keeps a leave from some other row from cancelling this row's timer.
 */
function onRowDragLeave(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir) return
  const row = event.currentTarget as HTMLElement | null
  const related = event.relatedTarget as HTMLElement | null
  if (related && row && row.contains(related)) {
    event.stopPropagation()
    return
  }
  const path = row?.dataset.path
  if (path && hoverDirPath.value !== path) {
    event.stopPropagation()
    return
  }
  dropTargetDir.value = null
  clearHoverOpen()
}

// --- Drop on the blank part of a column: move into that column's folder -----

const dropTargetDir = ref<string | null>(null)
const dropColIndex = ref<number | null>(null)

function onColDragOver(colIndex: number, event: DragEvent) {
  if (!isFileDrag(event)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  dropColIndex.value = colIndex
  clearHoverOpen()
}

function onColDrop(colIndex: number, event: DragEvent) {
  const col = columns.value[colIndex]
  if (!col || !isFileDrag(event)) return
  const sources = parseDragPaths(event)
  event.preventDefault()
  event.stopPropagation()
  dropColIndex.value = null
  dropTargetDir.value = null
  clearHoverOpen()
  if (canMoveTo(sources, col.path)) emit('move', sources, col.path)
}

function onColDragLeave(event: DragEvent) {
  const col = event.currentTarget as HTMLElement | null
  const related = event.relatedTarget as HTMLElement | null
  if (related && col && col.contains(related)) return
  dropColIndex.value = null
  clearHoverOpen()
}

/** Root-level fallback: a drop that slips past every column still must not navigate the webview. */
function onRootDragOver(event: DragEvent) {
  if (!isFileDrag(event)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}

function onRootDrop(event: DragEvent) {
  if (!isFileDrag(event)) return
  event.preventDefault()
  dropColIndex.value = null
  dropTargetDir.value = null
  clearHoverOpen()
}

function onRootDragLeave(event: DragEvent) {
  const root = event.currentTarget as HTMLElement | null
  const related = event.relatedTarget as HTMLElement | null
  if (related && root && root.contains(related)) return
  dropColIndex.value = null
  dropTargetDir.value = null
  clearHoverOpen()
}

// --- Zoom -------------------------------------------------------------------

/** Ctrl+wheel zooms the rows instead of scrolling, as in browsers. */
function onWheel(e: WheelEvent) {
  if (!e.ctrlKey && !e.metaKey) return
  e.preventDefault()
  view.nudge(e.deltaY < 0 ? 1 : -1)
}

onMounted(() => {
  // Registered by hand: a passive listener could not call preventDefault,
  // and the browser would zoom the whole page instead.
  rootRef.value?.addEventListener('wheel', onWheel, { passive: false })
})

onUnmounted(() => {
  rootRef.value?.removeEventListener('wheel', onWheel)
})

defineExpose({
  /** Re-list every column in place, keeping the opened chain. */
  async refresh() {
    const id = ++requestId
    const previous = columns.value
    const loaded = await Promise.all(previous.map((c) => loadColumn(c.path)))
    if (id !== requestId) return
    columns.value = loaded.map((col, i) => {
      col.selectedPath = previous[i]!.selectedPath
      return col
    })
    commitSelection()
  },
  /** Select everything in the last column — the folder being browsed. */
  selectAll() {
    const last = columns.value[columns.value.length - 1]
    if (!last) return
    selected.value = new Set(last.entries.map((e) => e.path))
    anchor.value = null
    commitSelection()
  },
  setPath,
})
</script>

<template>
  <div
    ref="rootRef"
    class="column-view"
    :style="{ '--row-scale': view.rowScale }"
    @contextmenu.self.prevent="onRootContext"
  >
    <div
      ref="columnsEl"
      class="columns"
      @dragover="onRootDragOver"
      @drop="onRootDrop"
      @dragleave="onRootDragLeave"
    >
      <div
        v-for="(col, colIndex) in columns"
        :key="col.path"
        class="column"
        :class="{ 'drop-target': dropColIndex === colIndex }"
        @click.self="onBlankClick"
        @contextmenu.self.prevent="onColContext(col, $event)"
        @dragover="onColDragOver(colIndex, $event)"
        @drop="onColDrop(colIndex, $event)"
        @dragleave="onColDragLeave"
      >
        <div v-if="col.loading" class="col-note">Loading…</div>
        <div v-else-if="col.error" class="col-note error">{{ col.error }}</div>
        <template v-else>
          <div
            v-for="entry in col.entries"
            :key="entry.path"
            class="entry"
            :class="{
              selected: selected.has(entry.path),
              opened: col.selectedPath === entry.path,
              'drop-dir': dropTargetDir === entry.path,
              hidden: entry.isHidden,
            }"
            draggable="true"
            :data-path="entry.path"
            @click="onEntryClick(colIndex, entry, $event)"
            @dblclick="onEntryDblClick(entry)"
            @contextmenu.prevent.stop="onEntryContext(colIndex, entry, $event)"
            @dragstart="onRowDragStart(entry, colIndex, $event)"
            @dragenter="onRowDragEnter(entry, colIndex, $event)"
            @dragover="onRowDragOver(entry, $event)"
            @drop="onRowDrop(entry, $event)"
            @dragleave="onRowDragLeave(entry, $event)"
            @dragend="dropTargetDir = null; dropColIndex = null; clearHoverOpen()"
          >
            <span class="icon">{{ fileIcon(entry.ext, entry.isDir) }}</span>
            <span class="label" :title="entry.name">{{ entry.name }}</span>
            <span v-if="entry.isSymlink" class="link-badge" title="Symbolic link">↗</span>
            <span v-if="entry.isDir" class="arrow">›</span>
            <span v-else class="size">{{ formatSize(entry.size) }}</span>
          </div>
          <div v-if="col.entries.length === 0" class="col-note">This folder is empty</div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
/*
 * Row metrics derive from --row-scale, like the details list, so Ctrl+wheel
 * tunes this view for eyesight or display DPI as well.
 */
.column-view {
  --row-scale: 1;
  height: 100%;
  overflow: hidden;
  user-select: none;
  background: var(--bg-primary);
}

.columns {
  height: 100%;
  display: flex;
  overflow-x: auto;
  overflow-y: hidden;
}

.column {
  flex: 0 0 calc(250px * var(--row-scale));
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  border-right: 1px solid var(--border);
  padding: calc(6px * var(--row-scale)) 0;
}

.column.drop-target {
  background: var(--bg-hover);
}

.entry {
  display: flex;
  align-items: center;
  gap: calc(7px * var(--row-scale));
  padding: calc(5px * var(--row-scale)) calc(12px * var(--row-scale));
  font-size: calc(13px * var(--row-scale));
  cursor: default;
  border-radius: 5px;
  margin: 1px calc(6px * var(--row-scale));
  color: var(--text-primary);
  user-select: none;
}

.entry:hover {
  background: var(--bg-hover);
}

.entry.selected {
  background: var(--bg-selected);
}

/* The folder a deeper column lists: muted highlight, accent arrow. */
.entry.opened:not(.selected) {
  background: var(--surface);
}

.entry.opened:not(.selected) .arrow {
  color: var(--accent);
}

.entry.drop-dir {
  outline: 1px dashed var(--accent);
  outline-offset: -2px;
}

.entry.hidden .label {
  opacity: 0.5;
}

.icon {
  flex-shrink: 0;
  font-size: calc(14px * var(--row-scale));
}

.label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.link-badge {
  color: var(--accent);
  font-size: calc(10px * var(--row-scale));
  flex-shrink: 0;
}

.arrow {
  color: var(--text-muted);
  font-size: calc(12px * var(--row-scale));
  flex-shrink: 0;
}

.size {
  color: var(--text-muted);
  font-size: calc(11px * var(--row-scale));
  flex-shrink: 0;
}

.col-note {
  display: flex;
  align-items: center;
  justify-content: center;
  height: calc(60px * var(--row-scale));
  color: var(--text-muted);
  font-size: calc(12px * var(--row-scale));
  padding: 0 calc(12px * var(--row-scale));
  text-align: center;
}

.col-note.error {
  color: var(--error);
}
</style>
