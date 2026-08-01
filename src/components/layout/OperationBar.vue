<script setup lang="ts">
import { ref } from 'vue'
import { useOperationsStore } from '@/stores/operations'
import { formatSize } from '@/composables/useFormat'
import type { JobState } from '@/types/operations'

const ops = useOperationsStore()
const expanded = ref(true)

const STATUS_ICONS: Record<JobState['status'], string> = {
  scanning: '🔍',
  running: '⏳',
  completed: '✓',
  failed: '⚠',
  cancelled: '⊘',
}

/**
 * Byte progress where it is meaningful, file counts otherwise: a tree of
 * tiny files can total near-zero bytes and would sit at 0% forever.
 */
function percent(job: JobState): number {
  if (job.status === 'completed') return 100
  if (job.totalBytes > 0) {
    return Math.min(100, Math.round((job.doneBytes / job.totalBytes) * 100))
  }
  if (job.totalFiles > 0) {
    return Math.min(100, Math.round((job.doneFiles / job.totalFiles) * 100))
  }
  return 0
}

function detail(job: JobState): string {
  if (job.status === 'scanning') return 'Counting files…'
  if (job.status === 'failed') return job.error
  if (job.status === 'cancelled') return 'Cancelled'
  if (job.status === 'completed') return `${job.doneFiles} items`

  const parts: string[] = []
  if (job.totalFiles > 0) parts.push(`${job.doneFiles} / ${job.totalFiles} files`)
  if (job.totalBytes > 0) {
    parts.push(`${formatSize(job.doneBytes)} / ${formatSize(job.totalBytes)}`)
  }
  if (job.bytesPerSec > 0) parts.push(`${formatSize(job.bytesPerSec)}/s`)
  const eta = remaining(job)
  if (eta) parts.push(eta)
  return parts.join(' · ')
}

function remaining(job: JobState): string {
  if (job.bytesPerSec <= 0 || job.totalBytes <= job.doneBytes) return ''
  const seconds = Math.round((job.totalBytes - job.doneBytes) / job.bytesPerSec)
  if (seconds < 60) return `${seconds}s left`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s left`
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m left`
}

function isActive(job: JobState): boolean {
  return job.status === 'scanning' || job.status === 'running'
}
</script>

<template>
  <div v-if="ops.hasJobs" class="op-bar">
    <button class="summary" @click="expanded = !expanded">
      <span class="chevron">{{ expanded ? '▾' : '▸' }}</span>
      <span class="summary-text">
        {{ ops.activeJobs.length }} operation{{ ops.activeJobs.length === 1 ? '' : 's' }}
      </span>
      <span class="track">
        <span class="fill" :style="{ width: ops.overallPercent + '%' }" />
      </span>
      <span class="summary-pct">{{ ops.overallPercent }}%</span>
      <span v-if="ops.totalSpeed > 0" class="summary-speed">
        {{ formatSize(ops.totalSpeed) }}/s
      </span>
    </button>

    <div v-if="expanded" class="jobs">
      <div v-for="job in ops.jobs" :key="job.id" class="job" :class="job.status">
        <span class="icon">{{ STATUS_ICONS[job.status] }}</span>
        <div class="body">
          <div class="line">
            <span class="label">{{ job.label }}</span>
            <span class="current">{{ job.current }}</span>
          </div>
          <span class="track thin">
            <span class="fill" :style="{ width: percent(job) + '%' }" />
          </span>
          <div class="detail">{{ detail(job) }}</div>
        </div>
        <button
          v-if="isActive(job)"
          class="action"
          title="Cancel"
          @click="ops.cancel(job.id)"
        >
          ✕
        </button>
        <button v-else class="action" title="Dismiss" @click="ops.dismiss(job.id)">×</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.op-bar {
  background: #181825;
  border-top: 1px solid #313244;
  flex-shrink: 0;
  max-height: 40vh;
  display: flex;
  flex-direction: column;
}

.summary {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  background: none;
  border: none;
  color: #a6adc8;
  font-family: inherit;
  font-size: 11px;
  padding: 5px 10px;
  cursor: pointer;
  flex-shrink: 0;
}

.summary:hover {
  background: #1e1e2e;
}

.chevron {
  color: #6c7086;
  width: 10px;
}

.summary-text {
  white-space: nowrap;
}

.summary-pct,
.summary-speed {
  color: #6c7086;
  white-space: nowrap;
}

.track {
  flex: 1;
  min-width: 60px;
  height: 4px;
  background: #313244;
  border-radius: 2px;
  overflow: hidden;
}

.track.thin {
  display: block;
  width: 100%;
  height: 3px;
  margin: 3px 0;
}

.fill {
  display: block;
  height: 100%;
  background: #89b4fa;
  transition: width 0.15s linear;
}

.jobs {
  overflow-y: auto;
  border-top: 1px solid #313244;
}

.job {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 10px;
  font-size: 11px;
  border-bottom: 1px solid #24243a;
}

.job.failed .fill {
  background: #f38ba8;
}

.job.completed .fill {
  background: #a6e3a1;
}

.job.cancelled .fill {
  background: #6c7086;
}

.icon {
  flex-shrink: 0;
  line-height: 1.6;
}

.job.failed .icon {
  color: #f38ba8;
}

.job.completed .icon {
  color: #a6e3a1;
}

.body {
  flex: 1;
  min-width: 0;
}

.line {
  display: flex;
  gap: 6px;
  min-width: 0;
}

.label {
  color: #cdd6f4;
  flex-shrink: 0;
}

.current {
  color: #a6adc8;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail {
  color: #6c7086;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.job.failed .detail {
  color: #f38ba8;
}

.action {
  background: none;
  border: none;
  color: #6c7086;
  cursor: pointer;
  font-size: 13px;
  padding: 2px 4px;
  flex-shrink: 0;
}

.action:hover {
  color: #f38ba8;
}
</style>
