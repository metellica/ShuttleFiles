import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { FileEntry, OpenWithSettings } from '@/types/filesystem'
import {
  defaultOpenWith,
  loadOpenWith,
  openEntryPath,
  saveOpenWith,
} from '@/composables/useTauri'

/**
 * The program text files open with, and which extensions count as text.
 *
 * Every way of opening a file in the app — double click, the context
 * menu, Enter — goes through {@link openEntry}, so one setting covers
 * them all without touching Windows' per-extension associations.
 */
export const useOpenWithStore = defineStore('openWith', () => {
  const textEditor = ref('')
  const textExtensions = ref<string[]>([])

  const extensionSet = computed(() => new Set(textExtensions.value))

  async function restore() {
    try {
      apply(await loadOpenWith())
    } catch (e) {
      console.error('Cannot restore open-with settings:', e)
    }
  }

  function apply(settings: OpenWithSettings) {
    textEditor.value = settings.textEditor
    textExtensions.value = settings.textExtensions
  }

  /** Rejects when the program does not exist, so the dialog can say so. */
  async function save(settings: OpenWithSettings) {
    apply(await saveOpenWith(settings))
  }

  async function defaults(): Promise<OpenWithSettings> {
    return defaultOpenWith()
  }

  /**
   * Files with no extension are matched on their name instead, which is
   * how `Makefile`, `Dockerfile` and `.gitignore` reach the editor.
   */
  function extensionOf(entry: FileEntry): string {
    return entry.ext || entry.name.replace(/^\./, '').toLowerCase()
  }

  function isText(entry: FileEntry): boolean {
    return !entry.isDir && extensionSet.value.has(extensionOf(entry))
  }

  /** The configured editor for text files; `undefined` = system default. */
  function programFor(entry: FileEntry): string | undefined {
    return textEditor.value && isText(entry) ? textEditor.value : undefined
  }

  function openEntry(entry: FileEntry): Promise<void> {
    return openEntryPath(entry.path, programFor(entry))
  }

  return {
    textEditor,
    textExtensions,
    restore,
    save,
    defaults,
    isText,
    programFor,
    openEntry,
  }
})
