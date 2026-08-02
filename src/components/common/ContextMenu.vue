<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch, nextTick } from 'vue'

export interface MenuItem {
  label?: string
  icon?: string
  danger?: boolean
  disabled?: boolean
  separator?: boolean
  /** Renders a tick in the icon column, for radio/toggle style entries. */
  checked?: boolean
  action?: () => void
  /** Turns the entry into a submenu; `action` is then ignored. */
  children?: MenuItem[]
}

const props = defineProps<{
  visible: boolean
  x: number
  y: number
  items: MenuItem[]
  /**
   * Set on a submenu. It suppresses the global listeners (the root menu
   * already owns them) and enables flipping to the far side of the parent
   * rather than sliding back over it.
   */
  nested?: boolean
  /** Right edge of the parent menu, used when a submenu has to flip left. */
  flipAnchor?: number
}>()

const emit = defineEmits<{ close: [] }>()

const menuRef = ref<HTMLElement | null>(null)
const pos = ref({ x: 0, y: 0 })
/** Index of the item whose submenu is open, if any. */
const openIndex = ref<number | null>(null)
const subPos = ref({ x: 0, y: 0, anchor: 0 })
/**
 * Moving the pointer from a parent entry to its submenu crosses the
 * sibling rows in between. Closing on the first sibling hover would make
 * the submenu unreachable, so the switch is delayed just long enough for
 * that diagonal travel.
 */
const SWITCH_DELAY_MS = 250
let closeTimer: ReturnType<typeof setTimeout> | null = null

function cancelPendingClose() {
  if (closeTimer) {
    clearTimeout(closeTimer)
    closeTimer = null
  }
}

/** Keep the menu inside the viewport, flipping like a native popup. */
async function place() {
  pos.value = { x: props.x, y: props.y }
  await nextTick()
  const el = menuRef.value
  if (!el) return
  const { width, height } = el.getBoundingClientRect()
  const margin = 4
  let { x, y } = pos.value
  if (x + width + margin > window.innerWidth) {
    // A submenu flips to the left of its parent; a root menu folds back
    // over its own anchor, which is where the pointer already is.
    x = Math.max(margin, (props.flipAnchor ?? x) - width)
  }
  if (y + height + margin > window.innerHeight) y = Math.max(margin, y - height)
  pos.value = { x, y }
}

watch(
  () => [props.visible, props.x, props.y],
  () => {
    cancelPendingClose()
    if (props.visible) place()
    else openIndex.value = null
  },
  // A submenu is mounted already visible and already positioned, so it
  // would otherwise never be placed and would render at the origin.
  { immediate: true }
)

function onGlobalPointerDown(e: MouseEvent) {
  if (!menuRef.value?.contains(e.target as Node)) emit('close')
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => {
  if (props.nested) return
  window.addEventListener('mousedown', onGlobalPointerDown, true)
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  cancelPendingClose()
  window.removeEventListener('mousedown', onGlobalPointerDown, true)
  window.removeEventListener('keydown', onKey)
})

/** Open `index`'s submenu flush with its row, overlapping the border by 1px. */
function openSubmenu(index: number, event: MouseEvent) {
  cancelPendingClose()
  const row = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const menu = menuRef.value?.getBoundingClientRect()
  subPos.value = {
    x: (menu?.right ?? row.right) - 1,
    y: row.top - 4,
    anchor: menu?.left ?? row.left,
  }
  openIndex.value = index
}

function onItemEnter(item: MenuItem, index: number, event: MouseEvent) {
  if (item.children?.length && !item.disabled) {
    openSubmenu(index, event)
    return
  }
  // Hovering a plain entry dismisses a sibling's submenu, as in a native
  // menu — but only once the pointer has settled, so passing over a row
  // on the way to the submenu does not close it.
  if (openIndex.value === null || closeTimer) return
  closeTimer = setTimeout(() => {
    closeTimer = null
    openIndex.value = null
  }, SWITCH_DELAY_MS)
}

function run(item: MenuItem, index: number, event: MouseEvent) {
  if (item.disabled || item.separator) return
  if (item.children?.length) {
    // Clicking a parent entry keeps the menu open; only leaves commit.
    openSubmenu(index, event)
    return
  }
  emit('close')
  item.action?.()
}
</script>

<template>
  <div
    v-if="visible"
    ref="menuRef"
    class="ctx-menu"
    :style="{ left: pos.x + 'px', top: pos.y + 'px' }"
    @contextmenu.prevent
  >
    <template v-for="(item, i) in items" :key="i">
      <div v-if="item.separator" class="ctx-sep" />
      <button
        v-else
        class="ctx-item"
        :class="{
          disabled: item.disabled,
          danger: item.danger,
          checked: item.checked,
          open: openIndex === i,
        }"
        :disabled="item.disabled"
        @click="run(item, i, $event)"
        @mouseenter="onItemEnter(item, i, $event)"
      >
        <span class="ctx-icon">{{ item.checked ? '✓' : (item.icon ?? '') }}</span>
        <span class="ctx-label">{{ item.label }}</span>
        <span v-if="item.children?.length" class="ctx-arrow">›</span>
      </button>
    </template>

    <!--
      Rendered inside this menu so the root's outside-click check, which
      walks the DOM, still recognises a click in a submenu as inside.
    -->
    <ContextMenu
      v-if="openIndex !== null && items[openIndex]?.children?.length"
      :visible="true"
      :x="subPos.x"
      :y="subPos.y"
      :flip-anchor="subPos.anchor"
      :items="items[openIndex]!.children!"
      nested
      @mouseenter="cancelPendingClose"
      @close="emit('close')"
    />
  </div>
</template>

<style scoped>
.ctx-menu {
  position: fixed;
  z-index: 300;
  min-width: 200px;
  background: #24243a;
  border: 1px solid #45475a;
  border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
  padding: 4px;
  display: flex;
  flex-direction: column;
  user-select: none;
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
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

.ctx-item:hover:not(.disabled) {
  background: #45475a;
}

.ctx-item.open {
  background: #45475a;
}

.ctx-item.disabled {
  color: #6c7086;
  cursor: default;
}

.ctx-item.danger {
  color: #f38ba8;
}

.ctx-item.checked {
  color: #89b4fa;
}

.ctx-icon {
  width: 16px;
  text-align: center;
  flex-shrink: 0;
}

.ctx-label {
  flex: 1;
}

.ctx-arrow {
  color: #6c7086;
  font-size: 14px;
  line-height: 1;
  flex-shrink: 0;
  margin-left: 8px;
}

.ctx-item.open .ctx-arrow {
  color: #cdd6f4;
}

.ctx-sep {
  height: 1px;
  background: #45475a;
  margin: 4px 6px;
}
</style>
