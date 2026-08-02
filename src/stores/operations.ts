import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import * as api from '@/composables/useTauri'
import type { JobKind, JobOptions, JobState } from '@/types/operations'

/** How long a successful job stays visible before it disappears. */
const AUTO_DISMISS_MS = 2500

export const useOperationsStore = defineStore('operations', () => {
  const jobs = ref<JobState[]>([])
  /**
   * Bumped whenever a job finishes, so open browsers can reload the
   * folder they are showing without knowing anything about jobs.
   */
  const completionTick = ref(0)

  const activeJobs = computed(() =>
    jobs.value.filter((j) => j.status === 'scanning' || j.status === 'running')
  )
  const hasJobs = computed(() => jobs.value.length > 0)

  /** Aggregate bar shown when the panel is collapsed. */
  const overallPercent = computed(() => {
    const active = activeJobs.value
    if (active.length === 0) return 0
    const total = active.reduce((sum, j) => sum + j.totalBytes, 0)
    const done = active.reduce((sum, j) => sum + j.doneBytes, 0)
    if (total > 0) return Math.min(100, Math.round((done / total) * 100))
    // A pure-metadata job (many tiny files) has no useful byte total.
    const files = active.reduce((sum, j) => sum + j.totalFiles, 0)
    const doneFiles = active.reduce((sum, j) => sum + j.doneFiles, 0)
    return files > 0 ? Math.min(100, Math.round((doneFiles / files) * 100)) : 0
  })

  const totalSpeed = computed(() =>
    activeJobs.value.reduce((sum, j) => sum + j.bytesPerSec, 0)
  )

  function dismiss(id: string) {
    jobs.value = jobs.value.filter((j) => j.id !== id)
    api.clearFinishedOperations().catch(() => {
      // Backend cleanup is best-effort; the UI list is what users see.
    })
  }

  function upsert(job: JobState) {
    const index = jobs.value.findIndex((j) => j.id === job.id)
    if (index === -1) jobs.value.push(job)
    else jobs.value[index] = job

    if (job.status === 'completed' || job.status === 'cancelled') {
      completionTick.value++
      // Failures stay on screen until dismissed so the error is readable.
      setTimeout(() => dismiss(job.id), AUTO_DISMISS_MS)
    } else if (job.status === 'failed') {
      completionTick.value++
    }
  }

  async function init() {
    jobs.value = await api.listOperations()
    return listen<JobState>('fileop:update', (event) => upsert(event.payload))
  }

  async function start(
    kind: JobKind,
    sources: string[],
    destDir?: string,
    options?: JobOptions
  ) {
    if (sources.length === 0) return
    await api.startOperation(kind, sources, destDir, options)
  }

  async function cancel(id: string) {
    await api.cancelOperation(id)
  }

  return {
    jobs,
    activeJobs,
    hasJobs,
    overallPercent,
    totalSpeed,
    completionTick,
    init,
    start,
    cancel,
    dismiss,
  }
})
