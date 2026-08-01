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
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 400;
}

.dialog {
  background: #24243a;
  border: 1px solid #45475a;
  border-radius: 8px;
  padding: 16px;
  width: 340px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
}

h3 {
  font-size: 13px;
  font-weight: 500;
  color: #cdd6f4;
  margin-bottom: 10px;
}

input {
  width: 100%;
  background: #181825;
  border: 1px solid #313244;
  border-radius: 4px;
  color: #cdd6f4;
  font-size: 12px;
  font-family: inherit;
  padding: 6px 8px;
  outline: none;
}

input:focus {
  border-color: #89b4fa;
}

.buttons {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 12px;
}

.btn {
  background: #313244;
  border: none;
  color: #cdd6f4;
  font-size: 12px;
  font-family: inherit;
  padding: 6px 14px;
  border-radius: 4px;
  cursor: pointer;
}

.btn:hover {
  background: #45475a;
}

.btn.primary {
  background: #89b4fa;
  color: #1e1e2e;
}

.btn.primary:hover {
  background: #6b8ae0;
}
</style>
