<script setup lang="ts">
import { ref } from 'vue'
import PathBar from '@/components/browser/PathBar.vue'
import DensityControl from '@/components/layout/DensityControl.vue'

const props = defineProps<{
  path: string
  canGoBack: boolean
  canGoForward: boolean
  canGoUp: boolean
  isFavorite: boolean
  filter: string
}>()

const emit = defineEmits<{
  back: []
  forward: []
  up: []
  refresh: []
  navigate: [path: string]
  'new-folder': []
  'toggle-favorite': []
  'update:filter': [value: string]
}>()

const pathBarRef = ref<InstanceType<typeof PathBar> | null>(null)

defineExpose({ focusPathBar: () => pathBarRef.value?.startEdit() })
</script>

<template>
  <div class="toolbar">
    <button class="nav-btn" :disabled="!props.canGoBack" title="Back (Alt+←)" @click="emit('back')">
      ←
    </button>
    <button
      class="nav-btn"
      :disabled="!props.canGoForward"
      title="Forward (Alt+→)"
      @click="emit('forward')"
    >
      →
    </button>
    <button class="nav-btn" :disabled="!props.canGoUp" title="Up (Alt+↑)" @click="emit('up')">
      ↑
    </button>
    <button class="nav-btn" title="Refresh (F5)" @click="emit('refresh')">⟳</button>

    <PathBar ref="pathBarRef" :path="props.path" @navigate="emit('navigate', $event)" />

    <input
      class="filter"
      :value="props.filter"
      placeholder="Filter…"
      spellcheck="false"
      @input="emit('update:filter', ($event.target as HTMLInputElement).value)"
    />

    <button
      class="nav-btn"
      :class="{ starred: props.isFavorite }"
      :disabled="!props.path"
      title="Add to favorites"
      @click="emit('toggle-favorite')"
    >
      {{ props.isFavorite ? '★' : '☆' }}
    </button>
    <button class="nav-btn" :disabled="!props.path" title="New folder" @click="emit('new-folder')">
      ＋
    </button>
    <DensityControl />
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 8px;
  background: #1e1e2e;
  border-bottom: 1px solid #313244;
  flex-shrink: 0;
}

.nav-btn {
  background: none;
  border: 1px solid transparent;
  color: #cdd6f4;
  cursor: pointer;
  font-size: 13px;
  width: 26px;
  height: 26px;
  border-radius: 4px;
  flex-shrink: 0;
  font-family: inherit;
}

.nav-btn:hover:not(:disabled) {
  background: #313244;
}

.nav-btn:disabled {
  color: #45475a;
  cursor: default;
}

.nav-btn.starred {
  color: #f9e2af;
}

.filter {
  width: 140px;
  height: 26px;
  flex-shrink: 0;
  background: #181825;
  border: 1px solid #313244;
  border-radius: 4px;
  color: #cdd6f4;
  font-size: 12px;
  font-family: inherit;
  padding: 0 8px;
  outline: none;
}

.filter:focus {
  border-color: #89b4fa;
}
</style>
