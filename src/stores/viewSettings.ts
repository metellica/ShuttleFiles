import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'

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

const STORAGE_KEY = 'shuttle-files:view'

function clamp(value: number): number {
  return Math.min(MAX_ROW_SCALE, Math.max(MIN_ROW_SCALE, value))
}

function load(): number {
  const raw = localStorage.getItem(STORAGE_KEY)
  if (!raw) return 1
  try {
    const parsed = JSON.parse(raw) as { rowScale?: unknown }
    return typeof parsed.rowScale === 'number' ? clamp(parsed.rowScale) : 1
  } catch {
    return 1
  }
}

export const useViewSettingsStore = defineStore('viewSettings', () => {
  const rowScale = ref(load())

  watch(rowScale, (value) => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ rowScale: value }))
  })

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

  return {
    rowScale,
    activePreset,
    percent,
    setScale,
    setPreset,
    nudge,
    reset,
  }
})
