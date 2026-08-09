<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch, nextTick } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useThemeStore } from '@/stores/theme'
import {
  clipboardReadText,
  clipboardWriteText,
  terminalReserve,
  terminalOpen,
  terminalInput,
  terminalResize,
  terminalClose,
} from '@/composables/useTauri'

const themeStore = useThemeStore()

/** Read a CSS custom property from the document root. */
function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function terminalTheme() {
  return {
    background: cssVar('--bg-secondary'),
    foreground: cssVar('--text-primary'),
    cursor: cssVar('--text-primary'),
    selectionBackground: cssVar('--text-disabled'),
  }
}

const props = defineProps<{
  shellId: string
  cwd: string
  /** Kept mounted while hidden so the shell survives tab switches. */
  visible: boolean
}>()

const emit = defineEmits<{ exited: [] }>()

const termEl = ref<HTMLDivElement | null>(null)
const error = ref('')
const exited = ref(false)

let term: Terminal | null = null
let fit: FitAddon | null = null
let terminalId: string | null = null
let terminalToken: string | null = null
let resizeObserver: ResizeObserver | null = null
let disposed = false
let selectionCopyTimer: ReturnType<typeof setTimeout> | null = null
const unlisteners: UnlistenFn[] = []

function b64encode(data: string): string {
  const bytes = new TextEncoder().encode(data)
  let bin = ''
  for (const b of bytes) bin += String.fromCharCode(b)
  return btoa(bin)
}

function b64decode(data: string): Uint8Array {
  const bin = atob(data)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return bytes
}

function onData(id: string, data: string) {
  if (id === terminalId) term?.write(b64decode(data))
}

function onExit(id: string) {
  if (id !== terminalId || exited.value) return
  exited.value = true
  term?.write('\r\n\x1b[90m[process exited]\x1b[0m\r\n')
  emit('exited')
}

async function addListener<T>(event: string, handler: (payload: T) => void) {
  const un = await listen<T>(event, (e) => handler(e.payload))
  if (disposed) un()
  else unlisteners.push(un)
}

function stopListening() {
  unlisteners.forEach((u) => u())
  unlisteners.length = 0
}

function writeSelection(text: string) {
  if (!text) return
  clipboardWriteText(text).catch((e) => console.error('Cannot copy terminal selection', e))
}

async function pasteClipboard() {
  if (!term || exited.value) return
  const text = await clipboardReadText()
  if (text) term.paste(text)
}

function onMouseDown(event: MouseEvent) {
  if (event.button !== 2) return
  event.preventDefault()
  event.stopPropagation()
  term?.focus()
  pasteClipboard().catch((e) => console.error('Cannot paste terminal clipboard', e))
}

onMounted(async () => {
  if (!termEl.value) return
  term = new Terminal({
    fontSize: 13,
    fontFamily: 'Consolas, "Cascadia Mono", Menlo, monospace',
    cursorBlink: true,
    theme: terminalTheme(),
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.open(termEl.value)
  fit.fit()
  term.onSelectionChange(() => {
    const selection = term?.getSelection() ?? ''
    if (!selection) return
    if (selectionCopyTimer) clearTimeout(selectionCopyTimer)
    selectionCopyTimer = setTimeout(() => writeSelection(selection), 120)
  })

  terminalId = crypto.randomUUID()
  await addListener<{ id: string; data: string }>('terminal:data', (p) => onData(p.id, p.data))
  await addListener<{ id: string }>('terminal:exit', (p) => onExit(p.id))
  if (disposed) return

  try {
    terminalToken = await terminalReserve(terminalId)
    if (disposed) {
      await terminalClose(terminalId, terminalToken)
      return
    }
    await terminalOpen(
      terminalId,
      terminalToken,
      props.shellId,
      props.cwd,
      term.cols,
      term.rows
    )
  } catch (e: any) {
    if (!disposed) {
      error.value = e?.toString() || 'Cannot open terminal'
      stopListening()
    }
    terminalId = null
    terminalToken = null
    return
  }
  if (disposed) {
    terminalClose(terminalId!, terminalToken!).catch(() => {})
    return
  }

  term.onData((data) => {
    if (terminalId && terminalToken && !exited.value) {
      terminalInput(terminalId, terminalToken, b64encode(data)).catch(() => {})
    }
  })
  term.onResize(({ cols, rows }) => {
    if (terminalId && terminalToken && !exited.value) {
      terminalResize(terminalId, terminalToken, cols, rows).catch(() => {})
    }
  })

  let raf = 0
  resizeObserver = new ResizeObserver(() => {
    cancelAnimationFrame(raf)
    raf = requestAnimationFrame(() => {
      if (props.visible) fit?.fit()
    })
  })
  if (termEl.value) resizeObserver.observe(termEl.value)
  if (props.visible) term.focus()
})

watch(
  () => props.visible,
  async (vis) => {
    if (vis) {
      await nextTick()
      fit?.fit()
      term?.focus()
    }
  }
)

onBeforeUnmount(() => {
  disposed = true
  if (selectionCopyTimer) clearTimeout(selectionCopyTimer)
  resizeObserver?.disconnect()
  stopListening()
  if (terminalId && terminalToken) terminalClose(terminalId, terminalToken).catch(() => {})
  term?.dispose()
})
</script>

<template>
  <div class="term-view" v-show="visible">
    <div v-if="error" class="term-error">{{ error }}</div>
    <div
      v-else
      ref="termEl"
      class="term-host"
      @mousedown.capture="onMouseDown"
      @contextmenu.prevent.stop
    />
  </div>
</template>

<style scoped>
.term-view {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.term-error {
  padding: 12px;
  color: var(--error);
  font-size: 12px;
}

.term-host {
  flex: 1;
  min-height: 0;
  padding: 4px 0 0 6px;
}

.term-host :deep(.xterm) {
  height: 100%;
}
</style>
