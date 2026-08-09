<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useOperationsStore } from '@/stores/operations'
import { COMPRESSION_LEVELS, CREATABLE_FORMATS } from '@/stores/archives'
import { archiveSuggestName } from '@/composables/useTauri'

const props = defineProps<{
  /** Files and folders to pack. */
  sources: string[]
  /** Folder the archive is created in. */
  dir: string
}>()
const emit = defineEmits<{ close: [] }>()

const ops = useOperationsStore()

const format = ref(CREATABLE_FORMATS[0]!.extension)
const level = ref(6)
const name = ref('')
const error = ref('')
const busy = ref(false)

const separator = computed(() => (props.dir.includes('\\') ? '\\' : '/'))

/** Ask Rust for a name so the same rules apply as when it renames. */
async function suggest() {
  try {
    name.value = await archiveSuggestName(props.dir, props.sources, format.value)
  } catch (e) {
    error.value = String(e)
  }
}

onMounted(suggest)

// Switching format renames the suggestion, but never a name the user typed.
watch(format, (next, previous) => {
  if (!name.value || name.value.endsWith(`.${previous}`)) {
    const stem = name.value.slice(0, name.value.length - previous.length - 1)
    name.value = stem ? `${stem}.${next}` : ''
    if (!stem) suggest()
  }
})

const singleFileOnly = computed(() => props.sources.length === 1)

async function create() {
  if (!name.value.trim()) return
  busy.value = true
  error.value = ''
  try {
    const archivePath = `${props.dir.replace(/[\\/]+$/, '')}${separator.value}${name.value.trim()}`
    await ops.start('compress', props.sources, props.dir, {
      archivePath,
      level: level.value,
    })
    emit('close')
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="overlay" @mousedown.self="emit('close')">
    <div class="dialog" @keydown.esc="emit('close')">
      <h3>Add to Archive</h3>

      <p class="summary">
        {{ props.sources.length }} item{{ props.sources.length === 1 ? '' : 's' }} →
        {{ props.dir }}
      </p>

      <label for="archive-name">Archive name</label>
      <input
        id="archive-name"
        v-model="name"
        spellcheck="false"
        autofocus
        @keydown.enter="create"
      />

      <div class="row">
        <div class="field">
          <label for="archive-format">Format</label>
          <select id="archive-format" v-model="format">
            <option v-for="f in CREATABLE_FORMATS" :key="f.extension" :value="f.extension">
              {{ f.label }}
            </option>
          </select>
        </div>
        <div class="field">
          <label for="archive-level">Compression</label>
          <select id="archive-level" v-model.number="level">
            <option v-for="l in COMPRESSION_LEVELS" :key="l.value" :value="l.value">
              {{ l.label }}
            </option>
          </select>
        </div>
      </div>

      <p v-if="!singleFileOnly" class="hint">
        Everything selected keeps its own name at the archive's root.
      </p>
      <p v-if="error" class="error">{{ error }}</p>

      <div class="buttons">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn primary" :disabled="busy || !name.trim()" @click="create">
          Create
        </button>
      </div>
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
  padding: 16px;
  width: 460px;
  box-shadow: 0 8px 32px var(--shadow-md);
}

h3 {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: 10px;
}

.summary {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

input,
select {
  width: 100%;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  font-family: inherit;
  padding: 6px 8px;
  outline: none;
}

input:focus,
select:focus {
  border-color: var(--accent);
}

.row {
  display: flex;
  gap: 10px;
  margin-top: 12px;
}

.field {
  flex: 1;
}

.hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 10px;
}

.error {
  font-size: 12px;
  color: var(--error);
  margin-top: 10px;
}

.buttons {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 14px;
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
  color: var(--text-disabled);
  cursor: default;
}

.btn.primary {
  background: var(--accent);
  color: var(--accent-text);
}

.btn.primary:hover:not(:disabled) {
  background: var(--accent-hover);
}
</style>
