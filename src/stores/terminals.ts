import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { TerminalEntry } from '@/types/terminal'

/** One open embedded terminal, bound to the browser tab it was opened from. */
export interface TerminalInst {
  /** Client-side key (component identity). */
  key: string
  /** Browser tab this terminal belongs to. */
  tabId: string
  /** Which shell type is running (e.g. "cmd", "powershell", "git-bash", "vs2022-cmd"). */
  shellId: string
  /** Working directory the terminal was opened in. */
  cwd: string
  /** Display title for the terminal tab. */
  title: string
}

export const useTerminalsStore = defineStore('terminals', () => {
  const terms = ref<TerminalInst[]>([])
  /** tabId -> key of the active terminal tab in that browser tab's panel. */
  const activeByTab = ref<Record<string, string>>({})

  function open(tabId: string, shellId: string, cwd: string, shellLabel: string) {
    const inst: TerminalInst = {
      key: crypto.randomUUID(),
      tabId,
      shellId,
      cwd,
      title: shellLabel,
    }
    terms.value.push(inst)
    activeByTab.value[tabId] = inst.key
  }

  function close(key: string) {
    const inst = terms.value.find((t) => t.key === key)
    terms.value = terms.value.filter((t) => t.key !== key)
    if (inst && activeByTab.value[inst.tabId] === key) {
      const remaining = terms.value.filter((t) => t.tabId === inst.tabId)
      const next = remaining[remaining.length - 1]
      if (next) activeByTab.value[inst.tabId] = next.key
      else delete activeByTab.value[inst.tabId]
    }
  }

  /** Close every terminal of a browser tab (the tab is being closed). */
  function closeForTab(tabId: string) {
    terms.value = terms.value.filter((t) => t.tabId !== tabId)
    delete activeByTab.value[tabId]
  }

  function setActive(tabId: string, key: string) {
    activeByTab.value[tabId] = key
  }

  const byTab = computed(() => {
    const map = new Map<string, TerminalInst[]>()
    for (const t of terms.value) {
      const list = map.get(t.tabId)
      if (list) list.push(t)
      else map.set(t.tabId, [t])
    }
    return map
  })

  return { terms, activeByTab, byTab, open, close, closeForTab, setActive }
})
