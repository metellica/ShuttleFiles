<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { breadcrumbs as fetchCrumbs, resolvePath } from '@/composables/useTauri'
import ContextMenu, { type MenuItem } from '@/components/common/ContextMenu.vue'

const props = defineProps<{ path: string }>()
const emit = defineEmits<{ navigate: [path: string] }>()

const crumbs = ref<[string, string][]>([])
const editing = ref(false)
const draft = ref('')
const invalid = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)
const ctx = ref({ visible: false, x: 0, y: 0, items: [] as MenuItem[] })

watch(
  () => props.path,
  async (p) => {
    crumbs.value = await fetchCrumbs(p)
  },
  { immediate: true }
)

async function startEdit() {
  draft.value = props.path
  invalid.value = false
  editing.value = true
  await nextTick()
  inputRef.value?.focus()
  inputRef.value?.select()
}

function cancelEdit() {
  editing.value = false
  invalid.value = false
}

async function commit() {
  try {
    const resolved = await resolvePath(draft.value)
    editing.value = false
    invalid.value = false
    emit('navigate', resolved)
  } catch {
    // Keep the text so the user can fix a typo instead of retyping.
    invalid.value = true
  }
}

async function copyPath() {
  try {
    await navigator.clipboard.writeText(props.path)
  } catch (e) {
    console.error('Cannot copy path:', e)
  }
}

async function pasteAndGo() {
  try {
    const text = await navigator.clipboard.readText()
    if (!text) return
    emit('navigate', await resolvePath(text))
  } catch (e) {
    console.error('Cannot paste path:', e)
  }
}

function openMenu(event: MouseEvent) {
  ctx.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
    items: [
      { label: 'Copy Path', icon: '📋', action: copyPath },
      { label: 'Paste & Go', icon: '📥', action: pasteAndGo },
      { separator: true },
      { label: 'Edit Path', icon: '✏️', action: startEdit },
    ],
  }
}

defineExpose({ startEdit })
</script>

<template>
  <div class="path-bar" :class="{ invalid }" @contextmenu.prevent="openMenu">
    <input
      v-if="editing"
      ref="inputRef"
      v-model="draft"
      class="path-input"
      spellcheck="false"
      placeholder="Type a path, e.g. C:\Users or \\server\share"
      @keydown.enter="commit"
      @keydown.esc="cancelEdit"
      @blur="cancelEdit"
    />
    <template v-else>
      <div class="crumbs" @click.self="startEdit">
        <template v-for="([label, target], i) in crumbs" :key="target">
          <span v-if="i > 0" class="sep">›</span>
          <button class="crumb" :title="target" @click="emit('navigate', target)">
            {{ label }}
          </button>
        </template>
      </div>
      <div class="actions">
        <button class="icon-btn" title="Copy path" @click="copyPath">📋</button>
        <button class="icon-btn" title="Paste & go" @click="pasteAndGo">📥</button>
        <button class="icon-btn" title="Edit path (Ctrl+L)" @click="startEdit">✏️</button>
      </div>
    </template>

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
.path-bar {
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
  height: 26px;
  background: #181825;
  border: 1px solid #313244;
  border-radius: 4px;
  padding: 0 4px;
  font-size: 12px;
}

.path-bar.invalid {
  border-color: #f38ba8;
}

.crumbs {
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  white-space: nowrap;
  cursor: text;
  height: 100%;
}

.crumbs::-webkit-scrollbar {
  height: 0;
}

.crumb {
  background: none;
  border: none;
  color: #cdd6f4;
  font-size: 12px;
  font-family: inherit;
  padding: 2px 5px;
  border-radius: 3px;
  cursor: pointer;
  white-space: nowrap;
}

.crumb:hover {
  background: #313244;
  color: #89b4fa;
}

.sep {
  color: #6c7086;
  flex-shrink: 0;
}

.path-input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: none;
  outline: none;
  color: #cdd6f4;
  font-size: 12px;
  font-family: inherit;
  padding: 0 4px;
}

.actions {
  display: flex;
  flex-shrink: 0;
}

.icon-btn {
  background: none;
  border: none;
  color: #a6adc8;
  cursor: pointer;
  font-size: 11px;
  padding: 2px 4px;
  border-radius: 3px;
}

.icon-btn:hover {
  background: #313244;
}
</style>
