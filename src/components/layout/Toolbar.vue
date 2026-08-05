<script setup lang="ts">
import { nextTick, ref } from 'vue'
import PathBar from '@/components/browser/PathBar.vue'
import DensityControl from '@/components/layout/DensityControl.vue'

const props = defineProps<{
  path: string
  canGoBack: boolean
  canGoForward: boolean
  canGoUp: boolean
  isFavorite: boolean
  /** False inside an archive, where nothing can be written. */
  canModify: boolean
  filter: string
  recursive: boolean
  searching: boolean
  matches: number
  truncated: boolean
  /** Whether the window is showing two panes side by side. */
  split: boolean
}>()

const emit = defineEmits<{
  back: []
  forward: []
  up: []
  refresh: []
  navigate: [path: string]
  'new-folder': []
  'toggle-favorite': []
  'toggle-split': []
  settings: []
  'update:filter': [value: string]
  'update:recursive': [value: boolean]
}>()

const pathBarRef = ref<InstanceType<typeof PathBar> | null>(null)
const filterRef = ref<HTMLInputElement | null>(null)

async function focusFilter(recursive?: boolean) {
  if (recursive !== undefined) emit('update:recursive', recursive)
  await nextTick()
  filterRef.value?.focus()
  filterRef.value?.select()
}

defineExpose({ focusPathBar: () => pathBarRef.value?.startEdit(), focusFilter })
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

    <div class="find">
      <input
        ref="filterRef"
        class="filter"
        :value="props.filter"
        :placeholder="props.recursive ? 'Find in tree…' : 'Fuzzy filter…'"
        spellcheck="false"
        @input="emit('update:filter', ($event.target as HTMLInputElement).value)"
        @keydown.esc="emit('update:filter', '')"
      />
      <span v-if="props.filter" class="hits">
        <template v-if="props.searching">…</template>
        <template v-else>{{ props.matches }}{{ props.truncated ? '+' : '' }}</template>
      </span>
      <button
        class="nav-btn recurse"
        :class="{ on: props.recursive }"
        :disabled="!props.path"
        :title="
          props.recursive
            ? 'Searching subfolders (Ctrl+Shift+F)'
            : 'Searching this folder only (Ctrl+Shift+F)'
        "
        @click="emit('update:recursive', !props.recursive)"
      >
        ⤶
      </button>
    </div>

    <button
      class="nav-btn"
      :class="{ starred: props.isFavorite }"
      :disabled="!props.path || !props.canModify"
      title="Add to favorites"
      @click="emit('toggle-favorite')"
    >
      {{ props.isFavorite ? '★' : '☆' }}
    </button>
    <button
      class="nav-btn"
      :disabled="!props.path || !props.canModify"
      title="New folder"
      @click="emit('new-folder')"
    >
      ＋
    </button>
    <DensityControl />
    <button
      class="nav-btn"
      :class="{ on: props.split }"
      :title="props.split ? 'Close split view (Ctrl+\\)' : 'Split view (Ctrl+\\)'"
      @click="emit('toggle-split')"
    >
      ◫
    </button>
    <button class="nav-btn" title="Settings" @click="emit('settings')">⚙</button>
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

.nav-btn.on {
  color: #89b4fa;
  background: #313244;
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
  padding-right: 34px;
  outline: none;
}

.filter:focus {
  border-color: #89b4fa;
}

.find {
  position: relative;
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.hits {
  position: absolute;
  right: 32px;
  font-size: 10px;
  color: #6c7086;
  pointer-events: none;
}

.recurse {
  font-size: 14px;
}

.recurse.on {
  color: #89b4fa;
  background: #313244;
}
</style>
