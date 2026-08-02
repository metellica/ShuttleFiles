import { ref, shallowRef } from 'vue'
import type { SearchHit } from '@/types/filesystem'
import { cancelSearch, fuzzyFind } from '@/composables/useTauri'

/**
 * Debounce long enough that a fast typist does not start a walk per
 * keystroke, short enough that the list still feels live.
 */
const DEBOUNCE_MS = 90

/**
 * Drives the fuzzy finder for one browser pane.
 *
 * Scoring and traversal both live in Rust, so the toolbar filter and
 * "Find in Folder" rank identically; the only difference between them is
 * the `recursive` flag. Each pane owns a stable search id, which the
 * backend uses to cancel a superseded run.
 */
export function useFuzzySearch() {
  const id = crypto.randomUUID()

  const hits = shallowRef<SearchHit[]>([])
  const searching = ref(false)
  const total = ref(0)
  const truncated = ref(false)
  const error = ref('')

  let timer: ReturnType<typeof setTimeout> | null = null
  /** Guards against a slow search overwriting a newer one's results. */
  let generation = 0

  function clear() {
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
    generation++
    hits.value = []
    total.value = 0
    truncated.value = false
    searching.value = false
    error.value = ''
    cancelSearch(id).catch(() => {
      // Nothing was running; the cancel is best-effort by design.
    })
  }

  async function run(dir: string, query: string, recursive: boolean) {
    const mine = ++generation
    searching.value = true
    error.value = ''
    try {
      const result = await fuzzyFind(id, dir, query, recursive)
      if (mine !== generation) return
      // A cancelled run's partial results belong to the query that
      // replaced it, so they must not be shown.
      if (result.cancelled) return
      hits.value = result.hits
      total.value = result.total
      truncated.value = result.truncated
    } catch (e) {
      if (mine !== generation) return
      hits.value = []
      total.value = 0
      error.value = String(e)
    } finally {
      if (mine === generation) searching.value = false
    }
  }

  /** Debounced entry point; an empty query resets rather than searching. */
  function schedule(dir: string, query: string, recursive: boolean) {
    if (timer) clearTimeout(timer)
    if (!query.trim() || !dir) {
      clear()
      return
    }
    searching.value = true
    timer = setTimeout(() => {
      timer = null
      run(dir, query, recursive)
    }, DEBOUNCE_MS)
  }

  function dispose() {
    if (timer) clearTimeout(timer)
    cancelSearch(id).catch(() => {})
  }

  return { hits, searching, total, truncated, error, schedule, clear, dispose }
}
