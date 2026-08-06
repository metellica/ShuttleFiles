import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { loadViewSettings, saveViewSettings } from '@/composables/useTauri'
import type { ColumnWidths } from '@/types/filesystem'

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

export type ColumnKey = keyof ColumnWidths

/**
 * Column widths are stored unscaled: the file list multiplies them by the
 * row scale, so a dragged width keeps its proportions when the rows zoom.
 */
export const DEFAULT_COLUMN_WIDTHS: ColumnWidths = {
  name: 280,
  size: 90,
  type: 110,
  time: 140,
}

export const COLUMN_KEYS = Object.keys(DEFAULT_COLUMN_WIDTHS) as ColumnKey[]

/** Narrow enough to be useful, wide enough that the header stays grabbable. */
export const MIN_COLUMN_WIDTH = 56
export const MAX_COLUMN_WIDTH = 1200

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

function clampWidth(value: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, value)))
}

/** A width missing from an older view.json falls back to its default. */
function sanitizeWidths(widths: Partial<ColumnWidths> | undefined): ColumnWidths {
  const out = { ...DEFAULT_COLUMN_WIDTHS }
  for (const key of COLUMN_KEYS) {
    out[key] = clampWidth(Number(widths?.[key]), DEFAULT_COLUMN_WIDTHS[key])
  }
  return out
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
  const columnWidths = ref<ColumnWidths>({ ...DEFAULT_COLUMN_WIDTHS })
  // Until a divider is dragged the name column fills whatever is left, which
  // is the layout most people expect from a fresh window.
  const stretchName = ref(true)
  // Saving before the stored value has arrived would overwrite it with the default.
  let loaded = false

  function save() {
    if (!loaded) return
    saveViewSettings({
      rowScale: rowScale.value,
      splitRatio: splitRatio.value,
      columnWidths: { ...columnWidths.value },
      stretchName: stretchName.value,
    }).catch((e) => console.error('Cannot save view settings:', e))
  }

  async function restore() {
    try {
      const legacy = takeLegacyScale()
      if (legacy !== null) {
        rowScale.value = legacy
        loaded = true
        // Writing back turns a migrated localStorage value into view.json.
        await saveViewSettings({
          rowScale: legacy,
          splitRatio: splitRatio.value,
          columnWidths: { ...columnWidths.value },
          stretchName: stretchName.value,
        })
        return
      }
      const settings = await loadViewSettings()
      rowScale.value = clamp(settings.rowScale)
      splitRatio.value = clampRatio(settings.splitRatio)
      columnWidths.value = sanitizeWidths(settings.columnWidths)
      stretchName.value = settings.stretchName ?? true
    } catch (e) {
      console.error('Cannot restore view settings:', e)
    } finally {
      loaded = true
    }
  }

  watch([rowScale, splitRatio, stretchName], save)
  watch(columnWidths, save, { deep: true })

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

  /** Dragged header divider, in unscaled pixels. */
  function setColumnWidth(key: ColumnKey, value: number) {
    // A name column that has been given a width of its own can no longer
    // stretch, or dragging it narrower would be undone by the flex fill.
    if (key === 'name') stretchName.value = false
    columnWidths.value[key] = clampWidth(value, DEFAULT_COLUMN_WIDTHS[key])
  }

  function resetColumnWidth(key: ColumnKey) {
    columnWidths.value[key] = DEFAULT_COLUMN_WIDTHS[key]
    if (key === 'name') stretchName.value = true
  }

  function resetColumnWidths() {
    columnWidths.value = { ...DEFAULT_COLUMN_WIDTHS }
    stretchName.value = true
  }

  return {
    rowScale,
    splitRatio,
    columnWidths,
    stretchName,
    activePreset,
    percent,
    restore,
    setScale,
    setPreset,
    setSplitRatio,
    setColumnWidth,
    resetColumnWidth,
    resetColumnWidths,
    nudge,
    reset,
  }
})
