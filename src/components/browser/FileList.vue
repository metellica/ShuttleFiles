<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { FileEntry, SearchHit } from '@/types/filesystem'
import { fileIcon, formatSize, formatTime } from '@/composables/useFormat'
import { useViewSettingsStore } from '@/stores/viewSettings'

const props = defineProps<{
  entries: FileEntry[] | SearchHit[]
  loading: boolean
  error: string
  /** Entries are ranked search hits, so relevance is the natural order. */
  searchMode?: boolean
}>()

const emit = defineEmits<{
  open: [entry: FileEntry]
  context: [event: MouseEvent, entry: FileEntry | null]
  'selection-change': [paths: string[]]
}>()

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

// A new listing invalidates the old selection.
watch(
  () => props.entries,
  () => {
    selected.value = new Set()
    anchorIndex.value = null
    emit('selection-change', [])
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
  emit('selection-change', [...selected.value])
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
    :style="{ '--row-scale': view.rowScale }"
    @contextmenu.self.prevent="onBlankContext"
  >
    <div class="head">
      <button class="col name" @click="setSort('name')">
        Name<span v-if="sortKey === 'name'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
        <span v-else-if="sortKey === 'relevance'" class="rank-hint"> · by relevance</span>
      </button>
      <button class="col size" @click="setSort('size')">
        Size<span v-if="sortKey === 'size'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
      </button>
      <button class="col type" @click="setSort('ext')">
        Type<span v-if="sortKey === 'ext'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
      </button>
      <button class="col time" @click="setSort('modified')">
        Modified<span v-if="sortKey === 'modified'">{{ sortAsc ? ' ▲' : ' ▼' }}</span>
      </button>
    </div>

    <div class="body" @contextmenu.self.prevent="onBlankContext">
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
        :class="{ selected: selected.has(entry.path), hidden: entry.isHidden }"
        @click="onRowClick(entry, index, $event)"
        @dblclick="emit('open', entry)"
        @contextmenu.prevent.stop="onRowContext(entry, index, $event)"
      >
        <div class="col name">
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
        <div class="col size">{{ entry.isDir ? '' : formatSize(entry.size) }}</div>
        <div class="col type">{{ typeLabel(entry) }}</div>
        <div class="col time">{{ formatTime(entry.modified) }}</div>
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
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  user-select: none;
}

.head {
  display: flex;
  border-bottom: 1px solid #313244;
  background: #181825;
  flex-shrink: 0;
}

.head .col {
  background: none;
  border: none;
  color: #a6adc8;
  font-size: calc(11px * var(--row-scale));
  font-family: inherit;
  text-align: left;
  padding: calc(6px * var(--row-scale)) calc(8px * var(--row-scale));
  cursor: pointer;
}

.head .col:hover {
  color: #89b4fa;
}

.body {
  flex: 1;
  overflow-y: auto;
}

.row {
  display: flex;
  align-items: center;
  font-size: calc(12px * var(--row-scale));
  color: #cdd6f4;
  cursor: default;
  border-bottom: 1px solid transparent;
}

.row:hover {
  background: #242438;
}

.row.selected {
  background: #2c3a5c;
}

.row.hidden .label {
  opacity: 0.5;
}

.col {
  padding: calc(4px * var(--row-scale)) calc(8px * var(--row-scale));
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col.name {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: calc(6px * var(--row-scale));
}

.col.size {
  width: calc(90px * var(--row-scale));
  text-align: right;
  color: #a6adc8;
  flex-shrink: 0;
}

.col.type {
  width: calc(110px * var(--row-scale));
  color: #a6adc8;
  flex-shrink: 0;
}

.col.time {
  width: calc(140px * var(--row-scale));
  color: #a6adc8;
  flex-shrink: 0;
}

.icon {
  flex-shrink: 0;
}

.label {
  overflow: hidden;
  text-overflow: ellipsis;
}

.mark {
  color: #f9e2af;
  font-weight: 600;
}

.rank-hint {
  color: #6c7086;
  font-size: 0.9em;
}

.link-badge {
  color: #89b4fa;
  font-size: calc(10px * var(--row-scale));
}

.notice {
  padding: 24px;
  text-align: center;
  color: #6c7086;
  font-size: 13px;
}

.notice.error {
  color: #f38ba8;
}
</style>
