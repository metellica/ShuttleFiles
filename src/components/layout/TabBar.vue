<script setup lang="ts">
import { computed, ref } from 'vue'
import { hasStrayed, useTabsStore, type Pane, type Tab, type TabLock } from '@/stores/tabs'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'

const props = defineProps<{ pane: Pane }>()
const emit = defineEmits<{ 'new-tab': [] }>()
const tabsStore = useTabsStore()

/** Only the focused side is drawn at full strength when split. */
const focused = computed(() => !tabsStore.split || tabsStore.activePaneId === props.pane.id)

const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)
const fileDragTabId = ref<string | null>(null)
const FILE_DND_MIME = 'application/x-shuttle-files-paths'

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
      ...(tabsStore.split
        ? [
            {
              label: 'Move to Other Side',
              icon: '⇄',
              action: () => tabsStore.moveTabToOtherPane(tab.id),
            },
          ]
        : []),
      { label: 'Close Tab', icon: '×', action: () => tabsStore.closeTab(tab.id, true) },
      {
        label: 'Close Other Tabs',
        icon: '⊗',
        disabled: props.pane.tabs.length < 2,
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

function hasFileDrag(event: DragEvent): boolean {
  const types = event.dataTransfer?.types
  return !!types && Array.from(types).includes(FILE_DND_MIME)
}

function onDragOver(index: number, tabId: string, event: DragEvent) {
  if (dragIndex.value !== null) {
    event.preventDefault()
    dropIndex.value = index
    return
  }
  if (!hasFileDrag(event)) return
  event.preventDefault()
  fileDragTabId.value = tabId
  if (props.pane.activeTabId !== tabId) tabsStore.setActiveTab(tabId)
}

function onDrop(index: number, tabId: string, event: DragEvent) {
  if (dragIndex.value !== null) tabsStore.moveTab(props.pane.id, dragIndex.value, index)
  else if (hasFileDrag(event)) {
    event.preventDefault()
    tabsStore.setActiveTab(tabId)
  }
  dragIndex.value = null
  dropIndex.value = null
  fileDragTabId.value = null
}

function onDragEnd() {
  dragIndex.value = null
  dropIndex.value = null
  fileDragTabId.value = null
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
  <div class="tab-bar" :class="{ unfocused: !focused }" @mousedown="tabsStore.setActivePane(props.pane.id)">
    <div
      v-for="(tab, index) in props.pane.tabs"
      :key="tab.id"
      class="tab"
      :class="{
        active: tab.id === props.pane.activeTabId,
        'drop-target': dropIndex === index && dragIndex !== index,
        'file-drag-target': fileDragTabId === tab.id && dragIndex === null,
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
      @dragover="onDragOver(index, tab.id, $event)"
      @drop="onDrop(index, tab.id, $event)"
      @dragend="onDragEnd"
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
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  height: 36px;
  align-items: stretch;
  overflow-x: auto;
  user-select: none;
  flex-shrink: 0;
  min-width: 0;
}

/* The side without the focus recedes rather than disappears. */
.tab-bar.unfocused .tab.active {
  box-shadow: inset 0 2px 0 var(--text-disabled);
  color: var(--text-secondary);
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
  border-right: 1px solid var(--border);
  font-size: 12px;
  color: var(--text-secondary);
  min-width: 130px;
  max-width: 220px;
}

.tab.active {
  background: var(--bg-primary);
  color: var(--text-primary);
  box-shadow: inset 0 2px 0 var(--accent);
}

.tab:hover {
  background: var(--bg-hover);
}

.tab.drop-target {
  outline: 1px dashed var(--accent);
  outline-offset: -2px;
}

.tab.file-drag-target {
  box-shadow: inset 0 -2px 0 var(--accent);
}

.tab.locked {
  box-shadow: inset 2px 0 0 var(--warning);
}

.tab.locked.active {
  box-shadow: inset 2px 0 0 var(--warning), inset 0 2px 0 var(--accent);
}

.tab.soft-locked {
  box-shadow: inset 2px 0 0 var(--accent);
}

.tab.soft-locked.active {
  box-shadow: inset 2px 0 0 var(--accent), inset 0 2px 0 var(--accent);
}

.tab-lock {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1;
}

.tab-lock-mark {
  color: var(--warning);
  font-weight: 700;
  margin-left: 1px;
}

.tab.strayed .tab-lock-mark {
  color: var(--warning);
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
  color: var(--text-muted);
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  padding: 2px;
}

.tab-close:hover {
  color: var(--error);
}

.tab-add {
  background: none;
  border: none;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 18px;
  padding: 0 12px;
}

.tab-add:hover {
  color: var(--text-primary);
}
</style>
