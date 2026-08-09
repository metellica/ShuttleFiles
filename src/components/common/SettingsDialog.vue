<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useOpenWithStore } from '@/stores/openWith'

const emit = defineEmits<{ close: [] }>()

const openWith = useOpenWithStore()

const editor = ref(openWith.textEditor)
const extensions = ref(openWith.textExtensions.join(' '))
const error = ref('')
const saving = ref(false)

onMounted(() => {
  editor.value = openWith.textEditor
  extensions.value = openWith.textExtensions.join(' ')
})

async function browse() {
  const picked = await openFileDialog({
    title: 'Choose the program for text files',
    multiple: false,
    directory: false,
    filters: [{ name: 'Programs', extensions: ['exe', 'cmd', 'bat', 'com'] }],
  })
  if (typeof picked === 'string') editor.value = picked
}

async function resetExtensions() {
  try {
    const defaults = await openWith.defaults()
    extensions.value = defaults.textExtensions.join(' ')
  } catch (e) {
    error.value = String(e)
  }
}

async function save() {
  saving.value = true
  error.value = ''
  try {
    await openWith.save({
      textEditor: editor.value,
      // Any of comma, whitespace or newline separates entries, so a
      // pasted list works whatever it was copied from.
      textExtensions: extensions.value.split(/[\s,]+/).filter(Boolean),
    })
    emit('close')
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="overlay" @mousedown.self="emit('close')">
    <div class="dialog" @keydown.esc="emit('close')">
      <h3>Settings</h3>

      <section>
        <label for="editor">Open text files with</label>
        <div class="row">
          <input
            id="editor"
            v-model="editor"
            spellcheck="false"
            placeholder="System default"
            @keydown.enter="save"
          />
          <button class="btn" @click="browse">Browse…</button>
          <button class="btn" :disabled="!editor" @click="editor = ''">Clear</button>
        </div>
        <p class="hint">
          A full path, or a program name Windows can resolve (<code>code</code>,
          <code>notepad</code>). Empty uses the system default. Applies to double click and
          the context menu's Open.
        </p>
      </section>

      <section>
        <div class="row between">
          <label for="exts">Text file extensions</label>
          <button class="link" @click="resetExtensions">Reset to defaults</button>
        </div>
        <textarea id="exts" v-model="extensions" spellcheck="false" rows="6" />
        <p class="hint">
          Separated by spaces or commas, without the dot. Names without an extension
          (<code>Makefile</code>, <code>.gitignore</code>) match on the name itself.
        </p>
      </section>

      <p v-if="error" class="error">{{ error }}</p>

      <div class="buttons">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn primary" :disabled="saving" @click="save">Save</button>
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
  width: 520px;
  max-height: 90vh;
  overflow: auto;
  box-shadow: 0 8px 32px var(--shadow-md);
}

h3 {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: 12px;
}

section {
  margin-bottom: 14px;
}

label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.row.between {
  justify-content: space-between;
}

input,
textarea {
  flex: 1;
  width: 100%;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  font-family: inherit;
  padding: 6px 8px;
  outline: none;
  resize: vertical;
}

textarea {
  font-family: 'Cascadia Code', Consolas, monospace;
  line-height: 1.5;
}

input:focus,
textarea:focus {
  border-color: var(--accent);
}

.hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 6px;
  line-height: 1.5;
}

code {
  font-family: 'Cascadia Code', Consolas, monospace;
  color: var(--text-secondary);
}

.error {
  font-size: 12px;
  color: var(--error);
  margin-bottom: 10px;
}

.buttons {
  display: flex;
  justify-content: flex-end;
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
  flex-shrink: 0;
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

.link {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  padding: 0;
  margin-bottom: 6px;
}

.link:hover {
  text-decoration: underline;
}
</style>
