<script setup lang="ts">
import { ref } from 'vue'
import { useTabsStore } from '@/stores/tabs'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'

const emit = defineEmits<{ 'new-tab': [] }>()
const tabsStore = useTabsStore()

const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

const ctx = ref({ visible: false, x: 0, y: 0, items: [] as MenuItem[] })

function openTabMenu(tabId: string, event: MouseEvent) {
  ctx.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
    items: [
      { label: 'Duplicate Tab', icon: '⧉', action: () => tabsStore.duplicateTab(tabId) },
      { separator: true },
      { label: 'Close Tab', icon: '×', action: () => tabsStore.closeTab(tabId) },
      {
        label: 'Close Other Tabs',
        icon: '⊗',
        disabled: tabsStore.tabs.length < 2,
        action: () => tabsStore.closeOthers(tabId),
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

/** Middle-click closes, matching browser behaviour. */
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
      }"
      draggable="true"
      :title="tab.path || 'This PC'"
      @click="tabsStore.setActiveTab(tab.id)"
      @mousedown="onMouseDown(tab.id, $event)"
      @contextmenu.prevent="openTabMenu(tab.id, $event)"
      @dragstart="onDragStart(index, $event)"
      @dragover="onDragOver(index, $event)"
      @drop="onDrop(index)"
      @dragend="dragIndex = null; dropIndex = null"
    >
      <span class="tab-icon">{{ tab.path ? '📁' : '🖥' }}</span>
      <span class="tab-label">{{ tab.title }}</span>
      <button class="tab-close" title="Close" @click.stop="tabsStore.closeTab(tab.id)">×</button>
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
