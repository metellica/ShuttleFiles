<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import {
  MAX_ROW_SCALE,
  MIN_ROW_SCALE,
  ROW_PRESETS,
  useViewSettingsStore,
} from '@/stores/viewSettings'

const view = useViewSettingsStore()
const open = ref(false)
const rootRef = ref<HTMLElement | null>(null)

function onGlobalPointerDown(e: MouseEvent) {
  if (!rootRef.value?.contains(e.target as Node)) open.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

onMounted(() => {
  window.addEventListener('mousedown', onGlobalPointerDown, true)
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  window.removeEventListener('mousedown', onGlobalPointerDown, true)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <div ref="rootRef" class="density">
    <button
      class="trigger"
      :class="{ active: open }"
      title="Row size (Ctrl+wheel, Ctrl+= / Ctrl+-)"
      @click="open = !open"
    >
      <span class="glyph">Aa</span>
      <span v-if="view.percent !== 100" class="badge">{{ view.percent }}%</span>
    </button>

    <div v-if="open" class="popover">
      <div class="title">Row size</div>

      <div class="presets">
        <button
          v-for="preset in ROW_PRESETS"
          :key="preset.id"
          class="preset"
          :class="{ selected: view.activePreset === preset.id }"
          @click="view.setPreset(preset.id)"
        >
          <span class="preset-glyph" :style="{ fontSize: 8 + preset.scale * 5 + 'px' }">A</span>
          {{ preset.label }}
        </button>
      </div>

      <input
        class="slider"
        type="range"
        :min="MIN_ROW_SCALE"
        :max="MAX_ROW_SCALE"
        step="0.01"
        :value="view.rowScale"
        @input="view.setScale(Number(($event.target as HTMLInputElement).value))"
      />

      <div class="footer">
        <span>{{ view.percent }}%</span>
        <button class="reset" :disabled="view.percent === 100" @click="view.reset()">
          Reset
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.density {
  position: relative;
  flex-shrink: 0;
}

.trigger {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 26px;
  padding: 0 6px;
  background: none;
  border: 1px solid transparent;
  border-radius: 4px;
  color: var(--text-primary);
  font-family: inherit;
  cursor: pointer;
}

.trigger:hover,
.trigger.active {
  background: var(--border);
}

.glyph {
  font-size: 12px;
  font-weight: 600;
  line-height: 1;
}

.badge {
  font-size: 9px;
  color: var(--accent);
  line-height: 1;
}

.popover {
  position: absolute;
  top: 30px;
  right: 0;
  z-index: 250;
  width: 208px;
  background: var(--bg-panel);
  border: 1px solid var(--text-disabled);
  border-radius: 6px;
  box-shadow: 0 4px 16px var(--shadow-sm);
  padding: 10px;
}

.title {
  font-size: 11px;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.presets {
  display: flex;
  gap: 4px;
  margin-bottom: 10px;
}

.preset {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 6px 2px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 10px;
  font-family: inherit;
  cursor: pointer;
}

.preset:hover {
  border-color: var(--accent);
}

.preset.selected {
  border-color: var(--accent);
  background: var(--bg-selected);
}

.preset-glyph {
  font-weight: 600;
  line-height: 1;
  height: 18px;
  display: flex;
  align-items: flex-end;
}

.slider {
  width: 100%;
  accent-color: var(--accent);
  cursor: pointer;
}

.footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-muted);
}

.reset {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  padding: 0;
}

.reset:disabled {
  color: var(--text-disabled);
  cursor: default;
}
</style>
