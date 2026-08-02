<script setup lang="ts">
import { ref } from 'vue'
import { hasStrayed, useTabsStore, type Tab, type TabLock } from '@/stores/tabs'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'

const emit = defineEmits<{ 'new-tab': [] }>()
const tabsStore = useTabsStore()

const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

const ctx = ref({ visible: false, x: 0, y: 0, items: [] as MenuItem[] })

const LOCK_LABELS: Record<TabLock, string> = {
  none: 'Unlocked',
  locked: 'Locked',
  'locked-allow-dirs': 'Locked, directory changes allowed',
}

/** Total Commander marks locked tabs with `*`, and a strayed soft lock with `!`. */
function lockBadge(tab: Tab): string {
  if (tab.lock === 'none') return ''
  return hasStrayed(tab) ? '!*' : '*'
}

function lockTitle(tab: Tab): string {
  const path = tab.path || 'This PC'
  if (tab.lock === 'none') return path
  const base = tab.lockedPath || 'This PC'
  const stray = hasStrayed(tab) ? ` — away from ${base}` : ''
  return `${path}\n${LOCK_LABELS[tab.lock]}: ${base}${stray}`
}

function setLock(tabId: string, mode: TabLock) {
  tabsStore.setLock(tabId, mode)
}

function openTabMenu(tab: Tab, event: MouseEvent) {
  ctx.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
    items: [
      { label: 'Duplicate Tab', icon: '⧉', action: () => tabsStore.duplicateTab(tab.id) },
      { separator: true },
      {
        label: 'Unlocked',
        icon: '🔓',
        checked: tab.lock === 'none',
        action: () => setLock(tab.id, 'none'),
      },
      {
        label: 'Lock Tab',
        icon: '🔒',
        checked: tab.lock === 'locked',
        action: () => setLock(tab.id, 'locked'),
      },
      {
        label: 'Lock Tab, Allow Dir Changes',
        icon: '🔐',
        checked: tab.lock === 'locked-allow-dirs',
        action: () => setLock(tab.id, 'locked-allow-dirs'),
      },
      ...(hasStrayed(tab)
        ? [
            {
              label: 'Back to Locked Folder',
              icon: '↺',
              action: () => tabsStore.navigate(tab.id, tab.lockedPath),
            },
          ]
        : []),
      { separator: true },
      { label: 'Close Tab', icon: '×', action: () => tabsStore.closeTab(tab.id, true) },
      {
        label: 'Close Other Tabs',
        icon: '⊗',
        disabled: tabsStore.tabs.length < 2,
        action: () => tabsStore.closeOthers(tab.id),
      },
    ],
  }
}

function onDragStart(index: number, event: DragEvent) {
  dragIndex.value = index
  event.dataTransfer?.setData('application/x-shuttle-tab', String(index))
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move'
}

function onDragOver(index: number, event: DragEvent) {
  if (dragIndex.value === null) return
  event.preventDefault()
  dropIndex.value = index
}

function onDrop(index: number) {
  if (dragIndex.value !== null) tabsStore.moveTab(dragIndex.value, index)
  dragIndex.value = null
  dropIndex.value = null
}

/** Middle-click closes, matching browser behaviour. Locked tabs are protected. */
function onMouseDown(tabId: string, event: MouseEvent) {
  if (event.button === 1) {
    event.preventDefault()
    tabsStore.closeTab(tabId)
  }
}
</script>

<template>
  <div class="tab-bar">
    <div
      v-for="(tab, index) in tabsStore.tabs"
      :key="tab.id"
      class="tab"
      :class="{
        active: tab.id === tabsStore.activeTabId,
        'drop-target': dropIndex === index && dragIndex !== index,
        locked: tab.lock === 'locked',
        'soft-locked': tab.lock === 'locked-allow-dirs',
        strayed: hasStrayed(tab),
      }"
      draggable="true"
      :title="lockTitle(tab)"
      @click="tabsStore.setActiveTab(tab.id)"
      @mousedown="onMouseDown(tab.id, $event)"
      @contextmenu.prevent="openTabMenu(tab, $event)"
      @dragstart="onDragStart(index, $event)"
      @dragover="onDragOver(index, $event)"
      @drop="onDrop(index)"
      @dragend="dragIndex = null; dropIndex = null"
    >
      <span v-if="tab.lock !== 'none'" class="tab-lock">
        {{ tab.lock === 'locked' ? '🔒' : '🔐' }}<span class="tab-lock-mark">{{ lockBadge(tab) }}</span>
      </span>
      <span class="tab-icon">{{ tab.path ? '📁' : '🖥' }}</span>
      <span class="tab-label">{{ tab.title }}</span>
      <button
        v-if="tab.lock === 'none'"
        class="tab-close"
        title="Close"
        @click.stop="tabsStore.closeTab(tab.id)"
      >
        ×
      </button>
    </div>
    <button class="tab-add" title="New tab (Ctrl+T)" @click="emit('new-tab')">+</button>

    <ContextMenu
      :visible="ctx.visible"
      :x="ctx.x"
      :y="ctx.y"
      :items="ctx.items"
      @close="ctx.visible = false"
    />
  </div>
</template>

<style scoped>
.tab-bar {
  display: flex;
  background: #181825;
  border-bottom: 1px solid #313244;
  height: 36px;
  align-items: stretch;
  overflow-x: auto;
  user-select: none;
  flex-shrink: 0;
}

.tab-bar::-webkit-scrollbar {
  height: 0;
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  cursor: pointer;
  border-right: 1px solid #313244;
  font-size: 12px;
  color: #a6adc8;
  min-width: 130px;
  max-width: 220px;
}

.tab.active {
  background: #1e1e2e;
  color: #cdd6f4;
  box-shadow: inset 0 2px 0 #89b4fa;
}

.tab:hover {
  background: #242438;
}

.tab.drop-target {
  outline: 1px dashed #89b4fa;
  outline-offset: -2px;
}

.tab.locked {
  box-shadow: inset 2px 0 0 #f9e2af;
}

.tab.locked.active {
  box-shadow: inset 2px 0 0 #f9e2af, inset 0 2px 0 #89b4fa;
}

.tab.soft-locked {
  box-shadow: inset 2px 0 0 #94e2d5;
}

.tab.soft-locked.active {
  box-shadow: inset 2px 0 0 #94e2d5, inset 0 2px 0 #89b4fa;
}

.tab-lock {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1;
}

.tab-lock-mark {
  color: #f9e2af;
  font-weight: 700;
  margin-left: 1px;
}

.tab.strayed .tab-lock-mark {
  color: #fab387;
}

.tab-icon {
  flex-shrink: 0;
  font-size: 11px;
}

.tab-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tab-close {
  background: none;
  border: none;
  color: #6c7086;
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  padding: 2px;
}

.tab-close:hover {
  color: #f38ba8;
}

.tab-add {
  background: none;
  border: none;
  color: #a6adc8;
  cursor: pointer;
  font-size: 18px;
  padding: 0 12px;
}

.tab-add:hover {
  color: #cdd6f4;
}
</style>
