<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useTabsStore } from '@/stores/tabs'
import { useTerminalsStore } from '@/stores/terminals'
import type { TerminalEntry } from '@/types/terminal'
import TerminalView from '@/components/terminal/TerminalView.vue'

const props = defineProps<{
  terminals: TerminalEntry[]
}>()

const emit = defineEmits<{ 'open-terminal': [shellId: string] }>()

const tabsStore = useTabsStore()
const terminalsStore = useTerminalsStore()

/** Terminals of the currently shown browser tab. */
const activeTabTerms = computed(() => {
  const tabId = tabsStore.activeTabId
  if (!tabId) return []
  return terminalsStore.byTab.get(tabId) ?? []
})

const activeKey = computed(() => {
  const tabId = tabsStore.activeTabId
  return tabId ? terminalsStore.activeByTab[tabId] : undefined
})

// Terminals die with their browser tab.
watch(
  () => tabsStore.panes.flatMap((pane) => pane.tabs.map((tab) => tab.id)),
  (ids) => {
    const alive = new Set(ids)
    for (const t of [...terminalsStore.terms]) {
      if (!alive.has(t.tabId)) terminalsStore.closeForTab(t.tabId)
    }
  }
)

// --- "Add terminal" dropdown ---
const addDropdownOpen = ref(false)
const addBtnRef = ref<HTMLElement | null>(null)

function toggleAddDropdown() {
  addDropdownOpen.value = !addDropdownOpen.value
}

function pickShell(shellId: string) {
  addDropdownOpen.value = false
  emit('open-terminal', shellId)
}

function onDocClick(e: MouseEvent) {
  if (!addBtnRef.value?.contains(e.target as Node)) {
    addDropdownOpen.value = false
  }
}

onMounted(() => window.addEventListener('mousedown', onDocClick, true))
onUnmounted(() => window.removeEventListener('mousedown', onDocClick, true))

// --- Drag-resizable panel height ---
const height = ref(280)
function onResizeStart(e: MouseEvent) {
  e.preventDefault()
  const startY = e.clientY
  const startH = height.value
  const max = Math.round(window.innerHeight * 0.8)
  function onMove(ev: MouseEvent) {
    height.value = Math.min(max, Math.max(120, startH + (startY - ev.clientY)))
  }
  function onUp() {
    document.removeEventListener('mousemove', onMove)
    document.removeEventListener('mouseup', onUp)
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
  }
  document.addEventListener('mousemove', onMove)
  document.addEventListener('mouseup', onUp)
  document.body.style.cursor = 'ns-resize'
  document.body.style.userSelect = 'none'
}
</script>

<template>
  <div class="terminal-panel" v-show="activeTabTerms.length > 0" :style="{ height: height + 'px' }">
    <div class="resize-handle" title="Drag to resize" @mousedown="onResizeStart" />
    <div class="term-header">
      <span class="term-icon">⌨</span>
      <div class="term-tabs">
        <div
          v-for="t in activeTabTerms"
          :key="t.key"
          class="term-tab"
          :class="{ active: t.key === activeKey }"
          :title="`${t.title} — ${t.cwd}`"
          @click="terminalsStore.setActive(t.tabId, t.key)"
        >
          <span class="term-tab-title">{{ t.title }}</span>
          <button
            class="term-tab-close"
            title="Close terminal"
            @click.stop="terminalsStore.close(t.key)"
          >
            ✕
          </button>
        </div>
        <div ref="addBtnRef" class="term-add-wrap">
          <button class="term-add" title="New terminal" @click="toggleAddDropdown">+</button>
          <div v-if="addDropdownOpen" class="term-add-menu">
            <template v-for="(t, i) in props.terminals" :key="t.id">
              <div
                v-if="i > 0 && props.terminals[i - 1]!.group !== t.group"
                class="term-add-sep"
              />
              <button class="term-add-item" @click="pickShell(t.id)">
                {{ t.label }}
              </button>
            </template>
          </div>
        </div>
      </div>
    </div>
    <!-- All terminals stay mounted; only the active one shows -->
    <TerminalView
      v-for="t in terminalsStore.terms"
      :key="t.key"
      :shell-id="t.shellId"
      :cwd="t.cwd"
      :visible="t.tabId === tabsStore.activeTabId && t.key === activeKey"
    />
  </div>
</template>

<style scoped>
.terminal-panel {
  display: flex;
  flex-direction: column;
  min-height: 120px;
  background: #181825;
  border-top: 1px solid #313244;
  flex-shrink: 0;
  position: relative;
}

.resize-handle {
  position: absolute;
  top: -3px;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
  z-index: 10;
}

.resize-handle:hover {
  background: rgba(137, 180, 250, 0.3);
}

.term-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 10px;
  background: #1e1e2e;
  border-bottom: 1px solid #313244;
  flex-shrink: 0;
}

.term-icon {
  font-size: 11px;
  flex-shrink: 0;
}

.term-tabs {
  display: flex;
  gap: 4px;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  align-items: center;
}

.term-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 4px 2px 10px;
  background: #24243a;
  border: 1px solid #313244;
  border-radius: 4px;
  font-size: 12px;
  color: #a6adc8;
  cursor: pointer;
  flex-shrink: 0;
  max-width: 220px;
  user-select: none;
}

.term-tab.active {
  background: #313244;
  color: #cdd6f4;
  border-color: #45475a;
}

.term-tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: monospace;
}

.term-tab-close {
  background: transparent;
  border: none;
  color: #6c7086;
  cursor: pointer;
  font-size: 10px;
  padding: 2px 4px;
  border-radius: 3px;
  flex-shrink: 0;
}

.term-tab-close:hover {
  background: #45475a;
  color: #f38ba8;
}

.term-add-wrap {
  position: relative;
  flex-shrink: 0;
}

.term-add {
  background: transparent;
  border: 1px solid #313244;
  border-radius: 4px;
  color: #a6adc8;
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 1px 8px;
}

.term-add:hover {
  background: #313244;
  color: #cdd6f4;
}

.term-add-menu {
  position: absolute;
  bottom: 100%;
  left: 0;
  margin-bottom: 4px;
  z-index: 200;
  min-width: 220px;
  background: #24243a;
  border: 1px solid #45475a;
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
  padding: 4px;
  display: flex;
  flex-direction: column;
}

.term-add-item {
  display: block;
  width: 100%;
  background: none;
  border: none;
  color: #cdd6f4;
  text-align: left;
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 4px;
  font-family: inherit;
}

.term-add-item:hover {
  background: #45475a;
}

.term-add-sep {
  height: 1px;
  background: #45475a;
  margin: 4px 6px;
}
</style>
