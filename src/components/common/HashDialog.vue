<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { HashAlgo, HashProgress, HashResult } from '@/types/filesystem'
import { cancelHash, startHash } from '@/composables/useTauri'
import { formatSize } from '@/composables/useFormat'

const props = defineProps<{ paths: string[]; algos?: HashAlgo[] }>()
const emit = defineEmits<{ close: [] }>()

const algos = ref<HashAlgo[]>(props.algos?.length ? [...props.algos] : ['md5', 'sha256'])
const results = ref<HashResult[]>([])
const progress = ref<HashProgress | null>(null)
const running = ref(false)
const expected = ref('')
const copied = ref('')

let jobId = ''
let unlisten: UnlistenFn[] = []
let copyTimer: ReturnType<typeof setTimeout> | null = null

const percent = computed(() => {
  const p = progress.value
  if (!p || !p.totalBytes) return 0
  return Math.min(100, Math.round((p.doneBytes / p.totalBytes) * 100))
})

/**
 * Comparing by eye is exactly the mistake checksums exist to prevent, so
 * a pasted digest is matched against every value on screen. Whitespace
 * and case vary between tools that print hashes, and neither is
 * significant.
 */
const expectedNormalised = computed(() => expected.value.trim().toLowerCase())

function isMatch(value: string): boolean {
  return !!value && !!expectedNormalised.value && value === expectedNormalised.value
}

const comparison = computed(() => {
  if (!expectedNormalised.value) return null
  const hit = results.value.find((r) => isMatch(r.md5) || isMatch(r.sha256))
  return hit ? { ok: true, name: hit.name } : { ok: false, name: '' }
})

async function start() {
  if (props.paths.length === 0 || algos.value.length === 0) return
  await stop()
  jobId = crypto.randomUUID()
  results.value = []
  progress.value = null
  running.value = true

  unlisten = [
    await listen<{ id: string; result: HashResult }>('hash:result', (e) => {
      if (e.payload.id === jobId) results.value = [...results.value, e.payload.result]
    }),
    await listen<HashProgress>('hash:progress', (e) => {
      if (e.payload.id === jobId) progress.value = e.payload
    }),
    await listen<{ id: string; cancelled: boolean }>('hash:finished', (e) => {
      if (e.payload.id !== jobId) return
      running.value = false
      progress.value = null
    }),
  ]

  try {
    await startHash(jobId, props.paths, algos.value)
  } catch (e) {
    running.value = false
    console.error('Cannot start hashing:', e)
  }
}

async function stop() {
  unlisten.forEach((fn) => fn())
  unlisten = []
  if (jobId && running.value) {
    await cancelHash(jobId).catch(() => {})
  }
  running.value = false
  progress.value = null
}

function toggleAlgo(algo: HashAlgo) {
  const next = algos.value.includes(algo)
    ? algos.value.filter((a) => a !== algo)
    : [...algos.value, algo]
  // At least one digest has to be asked for, or there is nothing to show.
  if (next.length > 0) algos.value = next
}

async function copy(value: string, key: string) {
  await navigator.clipboard.writeText(value)
  copied.value = key
  if (copyTimer) clearTimeout(copyTimer)
  copyTimer = setTimeout(() => (copied.value = ''), 1200)
}

/** All computed digests, in the format `sha256sum` and friends emit. */
async function copyAll() {
  const lines = results.value
    .filter((r) => !r.error)
    .flatMap((r) =>
      algos.value
        .map((a) => (a === 'md5' ? r.md5 : r.sha256))
        .filter(Boolean)
        .map((digest) => `${digest}  ${r.name}`)
    )
  await copy(lines.join('\n'), 'all')
}

async function close() {
  await stop()
  emit('close')
}

// Re-run when the selection or the requested digests change.
watch([() => props.paths, algos], start, { immediate: true, deep: true })

onUnmounted(() => {
  if (copyTimer) clearTimeout(copyTimer)
  stop()
})
</script>

<template>
  <div class="overlay" @mousedown.self="close">
    <div class="dialog" @keydown.esc="close">
      <header>
        <h3>Checksums</h3>
        <span class="count">{{ props.paths.length }} file{{ props.paths.length === 1 ? '' : 's' }}</span>
        <span class="spacer" />
        <label class="algo">
          <input type="checkbox" :checked="algos.includes('md5')" @change="toggleAlgo('md5')" />
          MD5
        </label>
        <label class="algo">
          <input
            type="checkbox"
            :checked="algos.includes('sha256')"
            @change="toggleAlgo('sha256')"
          />
          SHA-256
        </label>
      </header>

      <div v-if="running" class="progress">
        <div class="bar"><div class="fill" :style="{ width: percent + '%' }" /></div>
        <span class="progress-text">
          <template v-if="progress">
            {{ progress.index }}/{{ progress.total }} ·
            {{ formatSize(progress.doneBytes) }} / {{ formatSize(progress.totalBytes) }}
          </template>
          <template v-else>Starting…</template>
        </span>
      </div>

      <div class="results">
        <div v-if="results.length === 0 && !running" class="empty">Nothing to hash</div>
        <div v-for="item in results" :key="item.path" class="item">
          <div class="name" :title="item.path">{{ item.name }}</div>
          <div v-if="item.error" class="digest error">{{ item.error }}</div>
          <template v-else>
            <div class="meta">{{ formatSize(item.size) || '0 B' }}</div>
            <div v-if="item.md5" class="digest" :class="{ match: isMatch(item.md5) }">
              <span class="tag">MD5</span>
              <code>{{ item.md5 }}</code>
              <button class="copy" title="Copy" @click="copy(item.md5, item.path + 'md5')">
                {{ copied === item.path + 'md5' ? '✓' : '⧉' }}
              </button>
            </div>
            <div v-if="item.sha256" class="digest" :class="{ match: isMatch(item.sha256) }">
              <span class="tag">SHA-256</span>
              <code>{{ item.sha256 }}</code>
              <button class="copy" title="Copy" @click="copy(item.sha256, item.path + 'sha')">
                {{ copied === item.path + 'sha' ? '✓' : '⧉' }}
              </button>
            </div>
          </template>
        </div>
      </div>

      <div class="compare">
        <input
          v-model="expected"
          class="expected"
          spellcheck="false"
          placeholder="Paste a checksum to verify…"
        />
        <span v-if="comparison" class="verdict" :class="{ ok: comparison.ok }">
          {{ comparison.ok ? `✓ matches ${comparison.name}` : '✗ no match' }}
        </span>
      </div>

      <footer>
        <button v-if="running" class="btn" @click="stop">Cancel</button>
        <button v-else class="btn" :disabled="results.length === 0" @click="copyAll">
          {{ copied === 'all' ? 'Copied ✓' : 'Copy All' }}
        </button>
        <span class="spacer" />
        <button class="btn primary" @click="close">Close</button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: var(--overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 400;
}

.dialog {
  background: var(--bg-panel);
  border: 1px solid var(--text-disabled);
  border-radius: 8px;
  padding: 14px;
  width: 640px;
  max-width: 92vw;
  box-shadow: 0 8px 32px var(--shadow-md);
  display: flex;
  flex-direction: column;
  gap: 10px;
}

header {
  display: flex;
  align-items: center;
  gap: 10px;
}

h3 {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.count {
  font-size: 11px;
  color: var(--text-muted);
}

.spacer {
  flex: 1;
}

.algo {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}

.progress {
  display: flex;
  align-items: center;
  gap: 8px;
}

.bar {
  flex: 1;
  height: 4px;
  background: var(--bg-secondary);
  border-radius: 2px;
  overflow: hidden;
}

.fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.1s linear;
}

.progress-text {
  font-size: 10px;
  color: var(--text-muted);
  white-space: nowrap;
}

.results {
  max-height: 46vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.empty {
  color: var(--text-muted);
  font-size: 12px;
  text-align: center;
  padding: 16px;
}

.item {
  background: var(--accent-text);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 10px;
}

.name {
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  font-size: 10px;
  color: var(--text-muted);
  margin-bottom: 4px;
}

.digest {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 2px;
  padding: 2px 4px;
  border-radius: 4px;
}

.digest.match {
  background: var(--hash-highlight);
}

.digest.error {
  color: var(--error);
  font-size: 11px;
}

.tag {
  font-size: 9px;
  color: var(--text-muted);
  width: 52px;
  flex-shrink: 0;
}

code {
  font-family: 'Cascadia Mono', Consolas, monospace;
  font-size: 11px;
  color: var(--success);
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  user-select: text;
}

.digest.match code {
  color: var(--success);
  font-weight: 600;
}

.copy {
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 12px;
  padding: 0 2px;
  flex-shrink: 0;
}

.copy:hover {
  color: var(--accent);
}

.compare {
  display: flex;
  align-items: center;
  gap: 8px;
}

.expected {
  flex: 1;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-family: 'Cascadia Mono', Consolas, monospace;
  font-size: 11px;
  padding: 5px 8px;
  outline: none;
}

.expected:focus {
  border-color: var(--accent);
}

.verdict {
  font-size: 11px;
  color: var(--error);
  white-space: nowrap;
}

.verdict.ok {
  color: var(--success);
}

footer {
  display: flex;
  align-items: center;
  gap: 8px;
}

.btn {
  background: var(--border);
  border: none;
  color: var(--text-primary);
  font-size: 12px;
  font-family: inherit;
  padding: 6px 14px;
  border-radius: 4px;
  cursor: pointer;
}

.btn:hover:not(:disabled) {
  background: var(--text-disabled);
}

.btn:disabled {
  color: var(--text-muted);
  cursor: default;
}

.btn.primary {
  background: var(--accent);
  color: var(--accent-text);
}

.btn.primary:hover {
  background: var(--accent-hover);
}
</style>
