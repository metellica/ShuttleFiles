<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { FileEntry, SearchHit } from '@/types/filesystem'
import { ROOT } from '@/types/filesystem'
import { fileIcon, formatSize, formatTime } from '@/composables/useFormat'
import { COLUMN_KEYS, useViewSettingsStore, type ColumnKey } from '@/stores/viewSettings'

const props = defineProps<{
  entries: FileEntry[] | SearchHit[]
  currentPath?: string
  /** Folder ".." leads to; absent (or during a search) hides that row. */
  parentPath?: string | null
  /**
   * Path to auto-select after loading, when the entry exists in the listing.
   * Set when navigating UP so the child folder you left is highlighted,
   * letting you re-enter it with a single double-click.
   */
  focusPath?: string | null
  loading: boolean
  error: string
  /** Entries are ranked search hits, so relevance is the natural order. */
  searchMode?: boolean
}>()

const emit = defineEmits<{
  open: [entry: FileEntry]
  context: [event: MouseEvent, entry: FileEntry | null]
  'selection-change': [paths: string[]]
  move: [sources: string[], destDir: string]
}>()

const DND_MIME = 'application/x-shuttle-files-paths'

/** `relevance` keeps the backend's ranking; it only exists while searching. */
type SortKey = 'name' | 'size' | 'modified' | 'ext' | 'relevance'

const view = useViewSettingsStore()
const selected = ref<Set<string>>(new Set())
const anchorIndex = ref<number | null>(null)
const sortKey = ref<SortKey>('name')
const sortAsc = ref(true)
const rootRef = ref<HTMLElement | null>(null)

/** Entering or leaving a search resets to that mode's natural order. */
watch(
  () => props.searchMode,
  (searching) => {
    sortKey.value = searching ? 'relevance' : 'name'
    sortAsc.value = true
  },
  { immediate: true }
)

/** Split a label into matched / unmatched runs for highlighting. */
function segments(entry: FileEntry | SearchHit) {
  const hit = entry as SearchHit
  const label = hit.rel ?? entry.name
  if (!hit.positions?.length) return [{ text: label, hit: false }]

  const chars = [...label]
  const marked = new Set(hit.positions)
  const runs: { text: string; hit: boolean }[] = []
  for (let i = 0; i < chars.length; i++) {
    const on = marked.has(i)
    const last = runs[runs.length - 1]
    if (last && last.hit === on) last.text += chars[i]
    else runs.push({ text: chars[i]!, hit: on })
  }
  return runs
}

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

onBeforeUnmount(() => {
  rootRef.value?.removeEventListener('wheel', onWheel)
})

// A new listing resets selection, then auto-selects a row when appropriate:
// - `focusPath` (set when navigating UP) → the child folder you came from
// - `parentPath` ("..") → so you can go back with a double-click
watch(
  () => props.entries,
  () => {
    selected.value = new Set()
    anchorIndex.value = null
    if (props.searchMode || !props.entries.length) {
      commitSelection()
      return
    }
    const valid = new Set(props.entries.map((e) => e.path))
    if (props.focusPath && valid.has(props.focusPath)) {
      selected.value = new Set([props.focusPath])
      anchorIndex.value = props.entries.findIndex((e) => e.path === props.focusPath)
    } else if (props.parentPath && props.parentPath !== '') {
      selected.value = new Set([props.parentPath])
      anchorIndex.value = 0
    }
    commitSelection()
    // Auto-selected row may be off-screen in a long listing; bring it to
    // the middle of the pane so the context around it is visible too.
    nextTick(() => {
      const row = rootRef.value?.querySelector('.row.selected') as HTMLElement | null
      row?.scrollIntoView({ block: 'center' })
    })
  }
)

const sorted = computed(() => {
  const list = [...props.entries]
  // Ranked hits arrive best-first; re-sorting them would throw the
  // ranking away, which is the whole point of a fuzzy search.
  if (sortKey.value === 'relevance') return list
  const dir = sortAsc.value ? 1 : -1
  list.sort((a, b) => {
    // Directories always lead, regardless of the active sort column.
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
    switch (sortKey.value) {
      case 'size':
        return (a.size - b.size) * dir
      case 'modified':
        return (a.modified - b.modified) * dir
      case 'ext':
        return a.ext.localeCompare(b.ext) * dir
      default:
        return a.name.localeCompare(b.name, undefined, { numeric: true }) * dir
    }
  })
  return list
})

function setSort(key: SortKey) {
  if (sortKey.value === key) sortAsc.value = !sortAsc.value
  else {
    sortKey.value = key
    sortAsc.value = true
  }
}

function commitSelection() {
  // Only emit paths of real entries — ".." and stale focusPath are kept
  // for visual highlight only and must never reach copy/cut/delete/rename.
  const valid = new Set(sorted.value.map((e) => e.path))
  const paths = [...selected.value].filter((p) => valid.has(p))
  emit('selection-change', paths)
}

function onRowClick(entry: FileEntry, index: number, event: MouseEvent) {
  if (event.shiftKey && anchorIndex.value !== null) {
    const [from, to] = [anchorIndex.value, index].sort((a, b) => a - b) as [number, number]
    selected.value = new Set(sorted.value.slice(from, to + 1).map((e) => e.path))
  } else if (event.ctrlKey || event.metaKey) {
    const next = new Set(selected.value)
    next.has(entry.path) ? next.delete(entry.path) : next.add(entry.path)
    selected.value = next
    anchorIndex.value = index
  } else {
    selected.value = new Set([entry.path])
    anchorIndex.value = index
  }
  commitSelection()
}

function onRowContext(entry: FileEntry, index: number, event: MouseEvent) {
  // Right-clicking outside the selection selects the row first, as Explorer does.
  if (!selected.value.has(entry.path)) {
    selected.value = new Set([entry.path])
    anchorIndex.value = index
    commitSelection()
  }
  emit('context', event, entry)
}

function onBlankContext(event: MouseEvent) {
  selected.value = new Set()
  commitSelection()
  emit('context', event, null)
}

function typeLabel(entry: FileEntry): string {
  if (entry.isDir) return 'Folder'
  return entry.ext ? `${entry.ext.toUpperCase()} file` : 'File'
}

/**
 * A synthetic ".." row for stepping up a level. Kept out of `sorted` (and
 * out of `selected`) since it isn't a real entry — just a shortcut that
 * always sits above whatever the folder actually contains. Search hits
 * come from all over the tree, so a parent link makes no sense among them.
 */
const parentEntry = computed<FileEntry | null>(() => {
  if (props.searchMode || props.parentPath == null) return null
  return {
    name: '..',
    path: props.parentPath,
    isDir: true,
    isSymlink: false,
    isHidden: false,
    size: 0,
    modified: 0,
    ext: '',
  }
})

const dropTargetDir = ref<string | null>(null)

function normalizePath(path: string): string {
  return path.replace(/[\\/]+$/, '').replace(/\//g, '\\').toLowerCase()
}

/** Containing folder of a normalized path, or '' if it has no separator. */
function parentOf(normalizedPath: string): string {
  const idx = normalizedPath.lastIndexOf('\\')
  return idx === -1 ? '' : normalizedPath.slice(0, idx)
}

/**
 * Chromium only exposes the actual payload of `getData()` during
 * `dragstart` and `drop`; a `dragover` handler reading it always gets an
 * empty string. `types` stays readable throughout, so hover feedback
 * checks that instead of trying (and failing) to read the paths early.
 */
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
 * Stepping into a folder mid-drag — holding the button down over it
 * instead of releasing — is what lets a drop reach a destination several
 * levels deeper without a separate trip to get there first.
 */
function armHoverOpen(entry: FileEntry) {
  if (hoverDirPath.value === entry.path) return
  clearHoverOpen()
  hoverDirPath.value = entry.path
  hoverTimer = setTimeout(() => {
    hoverTimer = null
    hoverDirPath.value = null
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

function onRowDragStart(entry: FileEntry, index: number, event: DragEvent) {
  if (!selected.value.has(entry.path)) {
    selected.value = new Set([entry.path])
    anchorIndex.value = index
    commitSelection()
  }
  let sources = selected.value.has(entry.path) ? [...selected.value] : [entry.path]
  // Only real entries may be dragged — ".." and stale focusPath are excluded.
  const valid = new Set(sorted.value.map((e) => e.path))
  sources = sources.filter((p) => valid.has(p))
  if (!event.dataTransfer || sources.length === 0) return
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData(DND_MIME, JSON.stringify(sources))
  event.dataTransfer.setData('text/plain', sources.join('\n'))
}

function onRowDragOver(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir || !isFileDrag(event)) return
  event.preventDefault()
  // `dragover` bubbles continuously (many times a second) while the
  // pointer sits still over the row. Left unstopped, every one of those
  // ticks would also hit the body's `dragover` handler below and clear
  // the hover-open timer this row just armed, so it could never fire.
  event.stopPropagation()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}

function onRowDrop(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir) return
  const sources = parseDragPaths(event)
  event.preventDefault()
  dropTargetDir.value = null
  clearHoverOpen()
  if (canMoveTo(sources, entry.path)) emit('move', sources, entry.path)
}

function onRowDragEnter(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir || !isFileDrag(event)) return
  event.preventDefault()
  event.stopPropagation()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  dropTargetDir.value = entry.path
  armHoverOpen(entry)
}

/**
 * `dragleave` fires on every child element, not just the row boundary.
 * Checking `relatedTarget` keeps the drop highlight and hover timer alive
 * while the pointer is still somewhere inside the row. Bubbling is stopped
 * whenever the pointer is judged to still be inside *some* row, so a leave
 * that's purely internal reshuffling never reaches the body-level handler
 * below and cancels a timer it has no business touching.
 *
 * Non-directory rows never arm anything (`onRowDragEnter`/`onRowDragOver`
 * bail out for them too), so their leave events are left alone to bubble
 * normally — otherwise the body-level fallback would never learn the drag
 * left the list while it was last over a file row, and the drop highlight
 * would stick around forever.
 */
function onRowDragLeave(entry: FileEntry, event: DragEvent) {
  if (!entry.isDir) return
  const row = event.currentTarget as HTMLElement | null
  const related = event.relatedTarget as HTMLElement | null
  // Child-element ghost: the pointer is still inside the same row. This is
  // the common case — real cursors jitter across the icon/label/column
  // child elements even while "holding still", firing dragleave+dragenter
  // pairs constantly. Left unstopped, every one of those would bubble up
  // and wipe the hover-open timer the very tick after it was armed.
  if (related && row && row.contains(related)) {
    event.stopPropagation()
    return
  }
  // Only clear when the pointer leaves the row we are actually hovering.
  // Without this, the dragleave from some other row would cancel the timer
  // that dragenter just armed on the target row.
  const path = row?.dataset.path
  if (path && hoverDirPath.value !== path) {
    event.stopPropagation()
    return
  }
  dropTargetDir.value = null
  clearHoverOpen()
}

/**
 * Body-level fallback for drop-target highlighting and the hover timer.
 * Only fires when the pointer leaves the list entirely (or lands on a
 * non-row area like the blank space below the last entry) — moving
 * between rows is fully handled by `onRowDragLeave`/`onRowDragEnter`, so
 * this must not blindly clear on every bubbled `dragleave`.
 */
function onBodyDragLeave(event: DragEvent) {
  const body = event.currentTarget as HTMLElement | null
  const related = event.relatedTarget as HTMLElement | null
  if (related && body && body.contains(related)) return
  dropTargetDir.value = null
  clearHoverOpen()
}

function onBodyDragOver(event: DragEvent) {
  if (!props.currentPath || props.currentPath === ROOT || !isFileDrag(event)) return
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  dropTargetDir.value = props.currentPath
  clearHoverOpen()
}

function onBodyDrop(event: DragEvent) {
  if (!props.currentPath || props.currentPath === ROOT) return
  const sources = parseDragPaths(event)
  event.preventDefault()
  dropTargetDir.value = null
  clearHoverOpen()
  if (canMoveTo(sources, props.currentPath)) emit('move', sources, props.currentPath)
}

/*
 * Column layout. Widths are stored unscaled and multiplied by the row
 * scale here, so zooming the rows keeps the columns in proportion.
 */
const scaled = computed(
  () =>
    Object.fromEntries(
      COLUMN_KEYS.map((key) => [key, view.columnWidths[key] * view.rowScale])
    ) as Record<ColumnKey, number>
)

/** Once the columns outgrow the pane the list scrolls sideways instead of squeezing. */
const totalWidth = computed(() => COLUMN_KEYS.reduce((sum, key) => sum + scaled.value[key], 0))

function colStyle(key: ColumnKey) {
  const width = `${scaled.value[key]}px`
  // The stretched name column fills the pane; once dragged it keeps its width
  // and the filler at the end of the row takes the slack instead.
  return key === 'name' && view.stretchName
    ? { flex: `1 0 ${width}`, minWidth: width }
    : { flex: `0 0 ${width}`, width }
}

/** Absorbs the leftover width so no column has to stretch to fill the row. */
const fillerStyle = computed(() => ({ flex: view.stretchName ? '0 0 0px' : '1 0 0px' }))

const resizing = ref<ColumnKey | null>(null)
const headNameRef = ref<HTMLElement | null>(null)

/** Drag a header divider; the width it writes is what the next start restores. */
function startResize(key: ColumnKey, event: MouseEvent) {
  const startX = event.clientX
  // Undo the row scale so a pixel of pointer travel is a pixel on screen.
  const scale = view.rowScale || 1
  // A stretched name column is wider than its stored width, so the drag has
  // to start from what is actually on screen or the first pixels do nothing.
  const startWidth =
    key === 'name' && headNameRef.value
      ? headNameRef.value.getBoundingClientRect().width / scale
      : view.columnWidths[key]
  resizing.value = key

  const move = (e: MouseEvent) => view.setColumnWidth(key, startWidth + (e.clientX - startX) / scale)
  const stop = () => {
    window.removeEventListener('mousemove', move)
    window.removeEventListener('mouseup', stop)
    document.body.classList.remove('splitting')
    resizing.value = null
  }
  window.addEventListener('mousemove', move)
  window.addEventListener('mouseup', stop)
  document.body.classList.add('splitting')
}

defineExpose({
  selectAll: () => {
    selected.value = new Set(sorted.value.map((e) => e.path))
    commitSelection()
  },
})
</script>

<template>
  <div
    ref="rootRef"
    class="file-list"
    :style="{ '--row-scale': view.rowScale, '--total-width': `${totalWidth}px` }"
    @contextmenu.self.prevent="onBlankContext"
  >
    <div class="scroll" @contextmenu.self.prevent="onBlankContext">
      <div class="sheet">
        <div class="head">
          <div ref="headNameRef" class="col name" :style="colStyle('name')">
            <button class="sort" @click="setSort('name')">
              Name<span v-if="sortKey === 'name'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
              <span v-else-if="sortKey === 'relevance'" class="rank-hint"> · by relevance</span>
            </button>
            <span
              class="grip"
              :class="{ active: resizing === 'name' }"
              title="Drag to resize, double-click to reset"
              @mousedown.prevent.stop="startResize('name', $event)"
              @dblclick.stop="view.resetColumnWidth('name')"
            />
          </div>
          <div class="col size" :style="colStyle('size')">
            <button class="sort" @click="setSort('size')">
              Size<span v-if="sortKey === 'size'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
            </button>
            <span
              class="grip"
              :class="{ active: resizing === 'size' }"
              title="Drag to resize, double-click to reset"
              @mousedown.prevent.stop="startResize('size', $event)"
              @dblclick.stop="view.resetColumnWidth('size')"
            />
          </div>
          <div class="col type" :style="colStyle('type')">
            <button class="sort" @click="setSort('ext')">
              Type<span v-if="sortKey === 'ext'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
            </button>
            <span
              class="grip"
              :class="{ active: resizing === 'type' }"
              title="Drag to resize, double-click to reset"
              @mousedown.prevent.stop="startResize('type', $event)"
              @dblclick.stop="view.resetColumnWidth('type')"
            />
          </div>
          <div class="col time" :style="colStyle('time')">
            <button class="sort" @click="setSort('modified')">
              Modified<span v-if="sortKey === 'modified'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
            </button>
            <span
              class="grip"
              :class="{ active: resizing === 'time' }"
              title="Drag to resize, double-click to reset"
              @mousedown.prevent.stop="startResize('time', $event)"
              @dblclick.stop="view.resetColumnWidth('time')"
            />
          </div>
          <div class="col filler" :style="fillerStyle" />
        </div>

        <div
          class="body"
          @contextmenu.self.prevent="onBlankContext"
          @dragover="onBodyDragOver"
          @drop="onBodyDrop"
          @dragleave="onBodyDragLeave"
        >
          <div
            v-if="parentEntry"
            class="row parent-row"
            :data-path="parentEntry.path"
            :class="{ 'drop-dir': dropTargetDir === parentEntry.path, selected: selected.has(parentEntry.path) }"
            @dblclick="emit('open', parentEntry)"
            @dragenter="onRowDragEnter(parentEntry, $event)"
            @dragover="onRowDragOver(parentEntry, $event)"
            @drop="onRowDrop(parentEntry, $event)"
            @dragleave="onRowDragLeave(parentEntry, $event)"
            @dragend="dropTargetDir = null; clearHoverOpen()"
          >
            <div class="col name" :style="colStyle('name')">
              <span class="icon">{{ fileIcon(parentEntry.ext, parentEntry.isDir) }}</span>
              <span class="label">..</span>
            </div>
            <div class="col size" :style="colStyle('size')" />
            <div class="col type" :style="colStyle('type')">Folder</div>
            <div class="col time" :style="colStyle('time')" />
            <div class="col filler" :style="fillerStyle" />
          </div>

          <div v-if="loading" class="notice">Loading…</div>
          <div v-else-if="error" class="notice error">{{ error }}</div>
          <div v-else-if="sorted.length === 0" class="notice">
            {{ props.searchMode ? 'No matches' : 'This folder is empty' }}
          </div>

          <div
            v-for="(entry, index) in sorted"
            v-else
            :key="entry.path"
            class="row"
            :data-path="entry.path"
            :class="{
              selected: selected.has(entry.path),
              hidden: entry.isHidden,
              'drop-dir': dropTargetDir === entry.path,
            }"
            draggable="true"
            @click="onRowClick(entry, index, $event)"
            @dblclick="emit('open', entry)"
            @contextmenu.prevent.stop="onRowContext(entry, index, $event)"
            @dragstart="onRowDragStart(entry, index, $event)"
            @dragenter="onRowDragEnter(entry, $event)"
            @dragover="onRowDragOver(entry, $event)"
            @drop="onRowDrop(entry, $event)"
            @dragleave="onRowDragLeave(entry, $event)"
            @dragend="dropTargetDir = null; clearHoverOpen()"
          >
            <div class="col name" :style="colStyle('name')">
              <span class="icon">{{ fileIcon(entry.ext, entry.isDir) }}</span>
              <span class="label"
                ><span
                  v-for="(part, i) in segments(entry)"
                  :key="i"
                  :class="{ mark: part.hit }"
                  >{{ part.text }}</span
                ></span
              >
              <span v-if="entry.isSymlink" class="link-badge" title="Symbolic link">↗</span>
            </div>
            <div class="col size" :style="colStyle('size')">
              {{ entry.isDir ? '' : formatSize(entry.size) }}
            </div>
            <div class="col type" :style="colStyle('type')">{{ typeLabel(entry) }}</div>
            <div class="col time" :style="colStyle('time')">{{ formatTime(entry.modified) }}</div>
            <div class="col filler" :style="fillerStyle" />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/*
 * Every size below is derived from --row-scale so the list can be tuned
 * for eyesight or display DPI. Column widths scale too, otherwise the
 * larger text clips at the bigger settings.
 */
.file-list {
  --row-scale: 1;
  --total-width: 0px;
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  user-select: none;
}

/* One scroller for the header and the rows, so they never drift apart. */
.scroll {
  flex: 1;
  overflow: auto;
}

.sheet {
  display: flex;
  flex-direction: column;
  /* Wide enough for the columns, but never narrower than the pane. */
  width: max(100%, var(--total-width));
  min-height: 100%;
}

.head {
  display: flex;
  border-bottom: 1px solid var(--border);
  background: var(--bg-secondary);
  flex-shrink: 0;
  position: sticky;
  top: 0;
  z-index: 1;
}

.head .col {
  position: relative;
  display: flex;
  align-items: center;
  padding: 0;
  /*
   * Cells clip their text with overflow:hidden, which would swallow the grip
   * that hangs over the column edge. Only the sort button needs clipping.
   */
  overflow: visible;
}

.head .col.size {
  justify-content: flex-end;
}

.head .sort {
  flex: 1;
  min-width: 0;
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: calc(11px * var(--row-scale));
  font-family: inherit;
  text-align: inherit;
  padding: calc(6px * var(--row-scale)) calc(8px * var(--row-scale));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.head .col.size .sort {
  text-align: right;
}

.head .sort:hover {
  color: var(--accent);
}

/* Kept inside the cell so the last column's handle cannot fall off the pane. */
.grip {
  position: absolute;
  top: 0;
  right: 0;
  width: 10px;
  height: 100%;
  cursor: col-resize;
  z-index: 2;
}

.grip::after {
  content: '';
  position: absolute;
  top: 15%;
  right: 0;
  width: 1px;
  height: 70%;
  background: var(--border);
}

.grip:hover::after,
.grip.active::after {
  background: var(--accent);
  width: 2px;
}

.body {
  flex: 1;
}

.row {
  display: flex;
  align-items: center;
  font-size: calc(12px * var(--row-scale));
  color: var(--text-primary);
  cursor: default;
  border-bottom: 1px solid transparent;
}

.row:hover {
  background: var(--bg-hover);
}

.row.selected {
  background: var(--bg-selected);
}

.row.drop-dir {
  outline: 1px dashed var(--accent);
  outline-offset: -1px;
}

.row.hidden .label {
  opacity: 0.5;
}

.col {
  box-sizing: border-box;
  padding: calc(4px * var(--row-scale)) calc(8px * var(--row-scale));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col.name {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: calc(6px * var(--row-scale));
}

.col.size {
  text-align: right;
  color: var(--text-secondary);
}

.col.type {
  color: var(--text-secondary);
}

.col.time {
  color: var(--text-secondary);
}

.col.filler {
  padding: 0;
  min-width: 0;
}

.icon {
  flex-shrink: 0;
}

.label {
  overflow: hidden;
  text-overflow: ellipsis;
}

.mark {
  color: var(--warning);
  font-weight: 600;
}

.rank-hint {
  color: var(--text-muted);
  font-size: 0.9em;
}

.link-badge {
  color: var(--accent);
  font-size: calc(10px * var(--row-scale));
}

.notice {
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: 13px;
}

.notice.error {
  color: var(--error);
}
</style>
