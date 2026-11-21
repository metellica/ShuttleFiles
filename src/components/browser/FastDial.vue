<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { usePlacesStore } from '@/stores/places'
import { driveIcon, formatSize } from '@/composables/useFormat'
import type { RecentEntry } from '@/types/filesystem'

const emit = defineEmits<{ navigate: [path: string] }>()
const places = usePlacesStore()

const dragId = ref<string | null>(null)

/** The part of a path below its machine, which the group header already names. */
function relativePath(item: RecentEntry): string {
  return item.host ? item.path.slice(item.host.length).replace(/^[\\/]+/, '') : item.path
}

function idleDays(lastVisit: number): number {
  return Math.floor((Date.now() / 1000 - lastVisit) / 86_400)
}

onMounted(() => {
  places.refresh().catch((e) => console.error('Cannot load places:', e))
})

function usedPercent(total: number, free: number): number {
  if (!total) return 0
  return Math.min(100, Math.round(((total - free) / total) * 100))
}

async function onFavDrop(targetId: string) {
  const sourceId = dragId.value
  dragId.value = null
  if (!sourceId || sourceId === targetId) return
  const ids = places.favorites.map((f) => f.id)
  const from = ids.indexOf(sourceId)
  const to = ids.indexOf(targetId)
  if (from === -1 || to === -1) return
  const [moved] = ids.splice(from, 1)
  ids.splice(to, 0, moved!)
  await places.reorder(ids)
}
</script>

<template>
  <div class="fast-dial">
    <section>
      <h2>Drives</h2>
      <div class="grid">
        <button
          v-for="drive in places.drives"
          :key="drive.path"
          class="tile drive"
          @click="emit('navigate', drive.path)"
        >
          <span class="tile-icon">{{ driveIcon(drive.kind) }}</span>
          <span class="tile-label">{{ drive.label || drive.path }}</span>
          <span class="tile-sub">{{ drive.path.replace(/[\\/]+$/, '') }}</span>
          <template v-if="drive.totalBytes > 0">
            <span class="bar">
              <span
                class="bar-fill"
                :class="{ full: usedPercent(drive.totalBytes, drive.freeBytes) > 90 }"
                :style="{ width: usedPercent(drive.totalBytes, drive.freeBytes) + '%' }"
              />
            </span>
            <span class="tile-sub">
              {{ formatSize(drive.freeBytes) }} free of {{ formatSize(drive.totalBytes) }}
            </span>
          </template>
        </button>
      </div>
    </section>

    <section v-if="places.quickAccess.length">
      <h2>Quick Access</h2>
      <div class="grid">
        <button
          v-for="place in places.quickAccess"
          :key="place.path"
          class="tile"
          :title="place.path"
          @click="emit('navigate', place.path)"
        >
          <span class="tile-icon">{{ place.icon }}</span>
          <span class="tile-label">{{ place.name }}</span>
        </button>
      </div>
    </section>

    <section v-if="places.favorites.length">
      <h2>Favorites <small>drag to reorder</small></h2>
      <div class="grid">
        <div
          v-for="fav in places.favorites"
          :key="fav.id"
          class="tile fav"
          :class="{ dragging: dragId === fav.id }"
          :title="fav.path"
          draggable="true"
          @click="emit('navigate', fav.path)"
          @dragstart="dragId = fav.id"
          @dragover.prevent
          @drop="onFavDrop(fav.id)"
          @dragend="dragId = null"
        >
          <span class="tile-icon">{{ fav.icon || '📁' }}</span>
          <span class="tile-label">{{ fav.name }}</span>
          <button class="remove" title="Remove favorite" @click.stop="places.remove(fav.id)">
            ×
          </button>
        </div>
      </div>
    </section>

    <section v-if="places.recentGroups.length">
      <h2>
        Recent by machine <small>pin keeps a folder; the rest fades after a week or so</small>
        <button class="link" @click="places.clearRecent()">clear</button>
      </h2>
      <div class="groups">
        <details
          v-for="group in places.recentGroups"
          :key="group.host"
          class="group"
          :open="!group.dormant"
        >
          <summary>
            <span class="group-name">{{ group.host ? '🖧' : '💻' }} {{ group.label }}</span>
            <span class="group-sub">
              <template v-if="group.dormant">idle {{ idleDays(group.lastVisit) }} days · </template>
              {{ group.total }} folder{{ group.total === 1 ? '' : 's' }}
            </span>
          </summary>
          <ul>
            <li v-for="item in group.items" :key="item.path" class="row" :title="item.path">
              <button class="row-main" @click="emit('navigate', item.path)">
                <span class="row-name">{{ item.name }}</span>
                <span class="row-path">{{ relativePath(item) }}</span>
              </button>
              <button
                class="row-btn"
                :class="{ on: item.pinned }"
                title="Pin"
                @click="places.togglePin(item.path)"
              >
                📌
              </button>
              <button class="row-btn" title="Forget" @click="places.removeRecent(item.path)">
                ×
              </button>
            </li>
          </ul>
          <div v-if="group.total > group.items.length" class="group-more">
            +{{ group.total - group.items.length }} more — Ctrl+G to search
          </div>
        </details>
      </div>
    </section>
  </div>
</template>

<style scoped>
.fast-dial {
  height: 100%;
  overflow-y: auto;
  padding: 20px 24px 32px;
}

section {
  margin-bottom: 26px;
}

h2 {
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.6px;
  color: var(--text-secondary);
  margin-bottom: 10px;
  display: flex;
  align-items: baseline;
  gap: 8px;
}

h2 small {
  font-size: 10px;
  font-weight: 400;
  text-transform: none;
  letter-spacing: 0;
  color: var(--text-muted);
}

.link {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 10px;
  font-family: inherit;
  cursor: pointer;
  text-decoration: underline;
}

.link:hover {
  color: var(--accent);
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(148px, 1fr));
  gap: 10px;
}

.groups {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 10px;
  align-items: start;
}

.group {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 4px 6px;
}

.group summary {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  padding: 0 8px 6px;
  cursor: pointer;
  list-style: none;
}

.group summary::-webkit-details-marker {
  display: none;
}

.group:not([open]) summary {
  padding-bottom: 2px;
  opacity: 0.7;
}

.group-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.group-sub,
.group-more {
  font-size: 10px;
  color: var(--text-muted);
  white-space: nowrap;
}

.group-more {
  padding: 4px 8px 0;
}

.group ul {
  list-style: none;
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  border-radius: 6px;
}

.row:hover {
  background: var(--bg-hover);
}

.row-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
  background: none;
  border: none;
  padding: 4px 8px;
  text-align: left;
  color: var(--text-primary);
  font-family: inherit;
  cursor: pointer;
}

.row-name {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-path {
  font-size: 10px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-btn {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 11px;
  cursor: pointer;
  padding: 4px 5px;
  opacity: 0;
  filter: grayscale(1);
}

.row:hover .row-btn,
.row-btn.on {
  opacity: 1;
}

.row-btn.on {
  filter: none;
}

.row-btn:hover {
  color: var(--error);
}

.tile {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px;
  cursor: pointer;
  color: var(--text-primary);
  font-family: inherit;
  text-align: left;
  transition: border-color 0.12s, background 0.12s;
}

.tile:hover {
  background: var(--bg-hover);
  border-color: var(--accent);
}

.tile.dragging {
  opacity: 0.4;
}

.tile-icon {
  font-size: 20px;
}

.tile-label {
  font-size: 12px;
  font-weight: 500;
  width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tile-sub {
  font-size: 10px;
  color: var(--text-muted);
}

.bar {
  width: 100%;
  height: 4px;
  background: var(--border);
  border-radius: 2px;
  overflow: hidden;
  margin-top: 4px;
}

.bar-fill {
  display: block;
  height: 100%;
  background: var(--accent);
}

.bar-fill.full {
  background: var(--error);
}

.remove {
  position: absolute;
  top: 4px;
  right: 4px;
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  opacity: 0;
  padding: 2px 4px;
}

.tile:hover .remove {
  opacity: 1;
}

.remove:hover {
  color: var(--error);
}
</style>
