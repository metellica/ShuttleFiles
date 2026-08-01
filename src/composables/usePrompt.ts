import { ref } from 'vue'

/**
 * Single-instance text prompt. The WebView's `window.prompt` is not
 * available in Tauri and the dialog plugin has no text input, so the
 * app renders its own modal (see `PromptDialog.vue`).
 */
export interface PromptState {
  visible: boolean
  title: string
  value: string
  resolve: ((value: string | null) => void) | null
}

export const promptState = ref<PromptState>({
  visible: false,
  title: '',
  value: '',
  resolve: null,
})

export function promptText(title: string, initial = ''): Promise<string | null> {
  return new Promise((resolve) => {
    promptState.value = { visible: true, title, value: initial, resolve }
  })
}

export function resolvePrompt(value: string | null) {
  const { resolve } = promptState.value
  promptState.value = { visible: false, title: '', value: '', resolve: null }
  resolve?.(value)
}
