<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { promptState, resolvePrompt } from '@/composables/usePrompt'

const inputRef = ref<HTMLInputElement | null>(null)

watch(
  () => promptState.value.visible,
  async (visible) => {
    if (!visible) return
    await nextTick()
    inputRef.value?.focus()
    // Pre-select the name but not the extension, like Explorer's rename.
    const value = promptState.value.value
    const dot = value.lastIndexOf('.')
    inputRef.value?.setSelectionRange(0, dot > 0 ? dot : value.length)
  }
)

function confirm() {
  const value = promptState.value.value.trim()
  if (value) resolvePrompt(value)
}
</script>

<template>
  <div v-if="promptState.visible" class="overlay" @mousedown.self="resolvePrompt(null)">
    <div class="dialog">
      <h3>{{ promptState.title }}</h3>
      <input
        ref="inputRef"
        v-model="promptState.value"
        spellcheck="false"
        @keydown.enter="confirm"
        @keydown.esc="resolvePrompt(null)"
      />
      <div class="buttons">
        <button class="btn" @click="resolvePrompt(null)">Cancel</button>
        <button class="btn primary" @click="confirm">OK</button>
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
  width: 340px;
  box-shadow: 0 8px 32px var(--shadow-md);
}

h3 {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: 10px;
}

input {
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

input:focus {
  border-color: var(--accent);
}

.buttons {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 12px;
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

.btn:hover {
  background: var(--text-disabled);
}

.btn.primary {
  background: var(--accent);
  color: var(--accent-text);
}

.btn.primary:hover {
  background: var(--accent-hover);
}
</style>
