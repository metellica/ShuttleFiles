import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '@/composables/useTauri'

/**
 * File Copy/Cut backed by the **system** clipboard.
 *
 * On Windows this is `CF_HDROP` + `Preferred DropEffect`, so a Ctrl+C
 * here pastes into Explorer and a Ctrl+C in Explorer pastes here. The
 * WebView clipboard API cannot see file paths, hence the Rust detour.
 */
export const useClipboardStore = defineStore('clipboard', () => {
  /** Whether the system clipboard currently holds files. */
  const hasContent = ref(false)

  async function refresh() {
    try {
      hasContent.value = await api.clipboardHasFiles()
    } catch {
      hasContent.value = false
    }
  }

  async function copy(paths: string[]) {
    if (paths.length === 0) return
    await api.clipboardWriteFiles(paths, false)
    hasContent.value = true
  }

  async function cut(paths: string[]) {
    if (paths.length === 0) return
    await api.clipboardWriteFiles(paths, true)
    hasContent.value = true
  }

  async function read() {
    return api.clipboardReadFiles()
  }

  return { hasContent, refresh, copy, cut, read }
})
