<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from 'vue'
import PathBar from '@/components/browser/PathBar.vue'
import DensityControl from '@/components/layout/DensityControl.vue'
import { useThemeStore } from '@/stores/theme'
import type { TerminalEntry } from '@/types/terminal'

const theme = useThemeStore()

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
  terminals: TerminalEntry[]
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
  'open-terminal': [id: string]
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

const terminalDropdownOpen = ref(false)
const terminalBtnRef = ref<HTMLElement | null>(null)

function toggleTerminalDropdown() {
  terminalDropdownOpen.value = !terminalDropdownOpen.value
}

function pickTerminal(id: string) {
  terminalDropdownOpen.value = false
  emit('open-terminal', id)
}

// Close dropdown on outside click.
function onDocClick(e: MouseEvent) {
  if (!terminalBtnRef.value?.contains(e.target as Node)) {
    terminalDropdownOpen.value = false
  }
}
onMounted(() => window.addEventListener('mousedown', onDocClick, true))
onUnmounted(() => window.removeEventListener('mousedown', onDocClick, true))

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
    <div ref="terminalBtnRef" class="terminal-dropdown" :class="{ open: terminalDropdownOpen }">
      <button
        class="nav-btn"
        :disabled="!props.path || props.terminals.length === 0"
        title="Open Terminal"
        @click="toggleTerminalDropdown"
      >
        ⌨
      </button>
      <div v-if="terminalDropdownOpen" class="terminal-menu">
        <template v-for="(t, i) in props.terminals" :key="t.id">
          <div
            v-if="i > 0 && props.terminals[i - 1]!.group !== t.group"
            class="terminal-sep"
          />
          <button class="terminal-item" @click="pickTerminal(t.id)">
            {{ t.label }}
          </button>
        </template>
      </div>
    </div>
    <button
      class="nav-btn"
      :title="theme.theme === 'dark' ? 'Light mode' : 'Dark mode'"
      @click="theme.toggle()"
    >
      {{ theme.theme === 'dark' ? '☀' : '☾' }}
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
  background: var(--bg-primary);
  border-bottom: 1px solid var(--surface);
  flex-shrink: 0;
}

.nav-btn {
  background: none;
  border: 1px solid transparent;
  color: var(--text-primary);
  cursor: pointer;
  font-size: 13px;
  width: 26px;
  height: 26px;
  border-radius: 4px;
  flex-shrink: 0;
  font-family: inherit;
}

.nav-btn:hover:not(:disabled) {
  background: var(--surface);
}

.nav-btn:disabled {
  color: var(--text-disabled);
  cursor: default;
}

.nav-btn.starred {
  color: var(--warning);
}

.nav-btn.on {
  color: var(--accent);
  background: var(--surface);
}

.filter {
  width: 140px;
  height: 26px;
  flex-shrink: 0;
  background: var(--bg-secondary);
  border: 1px solid var(--surface);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: 12px;
  font-family: inherit;
  padding: 0 8px;
  padding-right: 34px;
  outline: none;
}

.filter:focus {
  border-color: var(--accent);
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
  color: var(--text-muted);
  pointer-events: none;
}

.recurse {
  font-size: 14px;
}

.recurse.on {
  color: var(--accent);
  background: var(--surface);
}

.terminal-dropdown {
  position: relative;
  flex-shrink: 0;
}

.terminal-menu {
  position: absolute;
  top: 100%;
  right: 0;
  margin-top: 4px;
  z-index: 200;
  min-width: 220px;
  background: var(--bg-panel);
  border: 1px solid var(--text-disabled);
  border-radius: 6px;
  box-shadow: 0 4px 16px var(--shadow-sm);
  padding: 4px;
  display: flex;
  flex-direction: column;
}

.terminal-item {
  display: block;
  width: 100%;
  background: none;
  border: none;
  color: var(--text-primary);
  text-align: left;
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 4px;
  font-family: inherit;
}

.terminal-item:hover {
  background: var(--text-disabled);
}

.terminal-sep {
  height: 1px;
  background: var(--text-disabled);
  margin: 4px 6px;
}
</style>
