import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { loadViewSettings, saveViewSettings } from '@/composables/useTauri'

/**
 * Row scale for the file list.
 *
 * Presets cover the common cases; the range in between is free so the
 * size can be dialled in with Ctrl+wheel or the slider — a fixed set of
 * three steps is not enough for everyone's eyesight or display DPI.
 */
export const MIN_ROW_SCALE = 0.75
export const MAX_ROW_SCALE = 2.5
export const ROW_SCALE_STEP = 0.08

/** Neither pane may be squeezed past this share of the window. */
export const MIN_SPLIT_RATIO = 0.15
export const MAX_SPLIT_RATIO = 0.85

export interface RowPreset {
  id: 'small' | 'medium' | 'large'
  label: string
  scale: number
}

export const ROW_PRESETS: RowPreset[] = [
  { id: 'small', label: 'Small', scale: 0.85 },
  { id: 'medium', label: 'Medium', scale: 1 },
  { id: 'large', label: 'Large', scale: 1.4 },
]

/** Legacy localStorage key, read once so existing users keep their zoom. */
const LEGACY_STORAGE_KEY = 'shuttle-files:view'

function clamp(value: number): number {
  return Math.min(MAX_ROW_SCALE, Math.max(MIN_ROW_SCALE, value))
}

function clampRatio(value: number): number {
  if (!Number.isFinite(value)) return 0.5
  return Math.min(MAX_SPLIT_RATIO, Math.max(MIN_SPLIT_RATIO, value))
}

/** Row scale saved by an older build that used the WebView's localStorage. */
function takeLegacyScale(): number | null {
  try {
    const raw = localStorage.getItem(LEGACY_STORAGE_KEY)
    if (!raw) return null
    localStorage.removeItem(LEGACY_STORAGE_KEY)
    const parsed = JSON.parse(raw) as { rowScale?: unknown }
    return typeof parsed.rowScale === 'number' ? clamp(parsed.rowScale) : null
  } catch {
    localStorage.removeItem(LEGACY_STORAGE_KEY)
    return null
  }
}

export const useViewSettingsStore = defineStore('viewSettings', () => {
  const rowScale = ref(1)
  const splitRatio = ref(0.5)
  // Saving before the stored value has arrived would overwrite it with the default.
  let loaded = false

  function save() {
    if (!loaded) return
    saveViewSettings({ rowScale: rowScale.value, splitRatio: splitRatio.value }).catch((e) =>
      console.error('Cannot save view settings:', e)
    )
  }

  async function restore() {
    try {
      const legacy = takeLegacyScale()
      if (legacy !== null) {
        rowScale.value = legacy
        loaded = true
        // Writing back turns a migrated localStorage value into view.json.
        await saveViewSettings({ rowScale: legacy, splitRatio: splitRatio.value })
        return
      }
      const settings = await loadViewSettings()
      rowScale.value = clamp(settings.rowScale)
      splitRatio.value = clampRatio(settings.splitRatio)
    } catch (e) {
      console.error('Cannot restore view settings:', e)
    } finally {
      loaded = true
    }
  }

  watch([rowScale, splitRatio], save)

  /** The preset the current scale corresponds to, if any. */
  const activePreset = computed(
    () => ROW_PRESETS.find((p) => Math.abs(p.scale - rowScale.value) < 0.005)?.id ?? null
  )

  const percent = computed(() => Math.round(rowScale.value * 100))

  function setScale(value: number) {
    rowScale.value = clamp(value)
  }

  function setPreset(id: RowPreset['id']) {
    const preset = ROW_PRESETS.find((p) => p.id === id)
    if (preset) rowScale.value = preset.scale
  }

  /** Ctrl+wheel / Ctrl+= / Ctrl+- adjustment. */
  function nudge(direction: 1 | -1) {
    // Round to the step grid so repeated nudges don't drift to values
    // like 1.0399999999999998.
    const next = Math.round((rowScale.value + direction * ROW_SCALE_STEP) * 100) / 100
    rowScale.value = clamp(next)
  }

  function reset() {
    rowScale.value = 1
  }

  /** Dragged splitter position, as the left pane's share of the width. */
  function setSplitRatio(value: number) {
    splitRatio.value = clampRatio(value)
  }

  return {
    rowScale,
    splitRatio,
    activePreset,
    percent,
    restore,
    setScale,
    setPreset,
    setSplitRatio,
    nudge,
    reset,
  }
})
