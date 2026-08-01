<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch, nextTick } from 'vue'

export interface MenuItem {
  label?: string
  icon?: string
  danger?: boolean
  disabled?: boolean
  separator?: boolean
  action?: () => void
}

const props = defineProps<{
  visible: boolean
  x: number
  y: number
  items: MenuItem[]
}>()

const emit = defineEmits<{ close: [] }>()

const menuRef = ref<HTMLElement | null>(null)
const pos = ref({ x: 0, y: 0 })

/** Keep the menu inside the viewport, flipping like a native popup. */
async function place() {
  pos.value = { x: props.x, y: props.y }
  await nextTick()
  const el = menuRef.value
  if (!el) return
  const { width, height } = el.getBoundingClientRect()
  const margin = 4
  let { x, y } = pos.value
  if (x + width + margin > window.innerWidth) x = Math.max(margin, x - width)
  if (y + height + margin > window.innerHeight) y = Math.max(margin, y - height)
  pos.value = { x, y }
}

watch(() => [props.visible, props.x, props.y], () => {
  if (props.visible) place()
})

function onGlobalPointerDown(e: MouseEvent) {
  if (!menuRef.value?.contains(e.target as Node)) emit('close')
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => {
  window.addEventListener('mousedown', onGlobalPointerDown, true)
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  window.removeEventListener('mousedown', onGlobalPointerDown, true)
  window.removeEventListener('keydown', onKey)
})

function run(item: MenuItem) {
  if (item.disabled || item.separator) return
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
        :class="{ disabled: item.disabled, danger: item.danger }"
        :disabled="item.disabled"
        @click="run(item)"
      >
        <span class="ctx-icon">{{ item.icon ?? '' }}</span>
        <span class="ctx-label">{{ item.label }}</span>
      </button>
    </template>
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

.ctx-item.disabled {
  color: #6c7086;
  cursor: default;
}

.ctx-item.danger {
  color: #f38ba8;
}

.ctx-icon {
  width: 16px;
  text-align: center;
  flex-shrink: 0;
}

.ctx-label {
  flex: 1;
}

.ctx-sep {
  height: 1px;
  background: #45475a;
  margin: 4px 6px;
}
</style>
