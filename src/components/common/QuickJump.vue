<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { usePlacesStore } from '@/stores/places'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; go: [path: string, newTab: boolean] }>()

const places = usePlacesStore()
const query = ref('')
const selected = ref(0)
const inputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLElement | null>(null)

interface Candidate {
  path: string
  name: string
  host: string
  pinned: boolean
  score: number
}

const LIMIT = 50

/** History plus favorites, so a starred folder is always reachable here too. */
const candidates = computed<Candidate[]>(() => {
  const seen = new Set<string>()
  const out: Candidate[] = []
  for (const r of places.recent) {
    seen.add(r.path.toLowerCase())
    out.push({ path: r.path, name: r.name, host: r.host, pinned: r.pinned, score: r.score })
  }
  for (const f of places.favorites) {
    if (seen.has(f.path.toLowerCase())) continue
    const host = f.path.startsWith('\\\\') ? '\\\\' + f.path.slice(2).split('\\')[0]!.toLowerCase() : ''
    out.push({ path: f.path, name: f.name, host, pinned: true, score: 0 })
  }
  return out
})

/**
 * `@nas docs` keeps folders on machines whose name contains "nas" (or
 * "local") and whose path contains "docs". Every plain term must appear
 * in the path; folders matching in their own name rank higher.
 */
const results = computed(() => {
  const terms = query.value.toLowerCase().split(/\s+/).filter(Boolean)
  const hosts = terms.filter((t) => t.startsWith('@')).map((t) => t.slice(1))
  const words = terms.filter((t) => !t.startsWith('@'))

  const scored: { c: Candidate; rank: number }[] = []
  for (const c of candidates.value) {
    const hostLabel = c.host ? c.host.toLowerCase() : 'local'
    if (!hosts.every((h) => hostLabel.includes(h))) continue
    const path = c.path.toLowerCase()
    const name = c.name.toLowerCase()
    if (!words.every((w) => path.includes(w))) continue
    const nameHits = words.filter((w) => name.includes(w)).length
    const rank = (c.pinned ? 1000 : 0) + (c.score + 0.1) * (1 + nameHits)
    scored.push({ c, rank })
  }
  scored.sort((a, b) => b.rank - a.rank)
  return scored.slice(0, LIMIT).map((s) => s.c)
})

watch(query, () => {
  selected.value = 0
})

watch(
  () => props.open,
  async (open) => {
    if (!open) return
    query.value = ''
    selected.value = 0
    places.refreshRecent().catch(() => {})
    await nextTick()
    inputRef.value?.focus()
  }
)

function move(delta: number) {
  const n = results.value.length
  if (!n) return
  selected.value = (selected.value + delta + n) % n
  nextTick(() => {
    listRef.value?.children[selected.value]?.scrollIntoView({ block: 'nearest' })
  })
}

function accept(index: number, newTab: boolean) {
  const hit = results.value[index]
  if (hit) emit('go', hit.path, newTab)
}

async function removeAt(index: number) {
  const hit = results.value[index]
  if (!hit || hit.pinned) return
  await places.removeRecent(hit.path).catch(() => {})
  const n = results.value.length
  if (selected.value >= n) selected.value = Math.max(0, n - 1)
}

function onDelete(e: KeyboardEvent) {
  // Plain Delete edits text; Ctrl+Delete always removes the selected entry.
  if (query.value && !e.ctrlKey) return
  e.preventDefault()
  removeAt(selected.value)
}

function relative(c: Candidate): string {
  return c.host ? c.path.slice(c.host.length).replace(/^[\\/]+/, '') : c.path
}
</script>

<template>
  <div v-if="open" class="overlay" @mousedown.self="emit('close')">
    <div class="palette" @keydown.esc="emit('close')">
      <input
        ref="inputRef"
        v-model="query"
        spellcheck="false"
        placeholder="Jump to a folder…  (@machine to narrow, Enter opens, Ctrl+Enter opens in a new tab)"
        @keydown.down.prevent="move(1)"
        @keydown.up.prevent="move(-1)"
        @keydown.enter.prevent="accept(selected, $event.ctrlKey || $event.metaKey)"
        @keydown.delete="onDelete"
      />
      <ul v-if="results.length" ref="listRef">
        <li
          v-for="(c, i) in results"
          :key="c.path"
          :class="{ active: i === selected }"
          :title="c.path"
          @mousemove="selected = i"
          @click="accept(i, $event.ctrlKey || $event.metaKey)"
        >
          <span class="name">{{ c.pinned ? '📌 ' : '' }}{{ c.name }}</span>
          <span class="host">{{ c.host || 'local' }}</span>
          <span class="path">{{ relative(c) }}</span>
          <button
            v-if="!c.pinned"
            class="del"
            title="Remove from history (Ctrl+Delete)"
            @click.stop="removeAt(i)"
          >×</button>
        </li>
      </ul>
      <div v-else class="empty">No matching folders in your history</div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: var(--overlay);
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding-top: 12vh;
  z-index: 400;
}

.palette {
  width: min(640px, 92vw);
  background: var(--bg-panel);
  border: 1px solid var(--text-disabled);
  border-radius: 8px;
  box-shadow: 0 8px 32px var(--shadow-md);
  overflow: hidden;
}

input {
  width: 100%;
  background: var(--bg-secondary);
  border: none;
  border-bottom: 1px solid var(--border);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  padding: 10px 12px;
  outline: none;
}

ul {
  list-style: none;
  margin: 0;
  padding: 4px;
  max-height: 50vh;
  overflow-y: auto;
}

li {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  grid-template-areas: 'name host' 'path path';
  column-gap: 8px;
  padding: 5px 8px;
  border-radius: 6px;
  cursor: pointer;
}

li.active {
  background: var(--bg-hover);
}

.name {
  grid-area: name;
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.host {
  grid-area: host;
  font-size: 10px;
  color: var(--accent);
}

.path {
  grid-area: path;
  font-size: 10px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.del {
  position: absolute;
  right: 6px;
  bottom: 3px;
  display: none;
  background: var(--bg-panel);
  border: none;
  border-radius: 4px;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 1px 6px;
}

li:hover .del,
li.active .del {
  display: block;
}

.del:hover {
  color: var(--error);
}

.empty {
  padding: 16px;
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
}
</style>
