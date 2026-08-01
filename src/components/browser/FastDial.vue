<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { usePlacesStore } from '@/stores/places'
import { driveIcon, formatSize } from '@/composables/useFormat'

const emit = defineEmits<{ navigate: [path: string] }>()
const places = usePlacesStore()

const dragId = ref<string | null>(null)

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

    <section v-if="places.frequent.length">
      <h2>
        Frequent
        <button class="link" @click="places.clearRecent()">clear</button>
      </h2>
      <div class="grid">
        <button
          v-for="item in places.frequent"
          :key="item.path"
          class="tile"
          :title="item.path"
          @click="emit('navigate', item.path)"
        >
          <span class="tile-icon">🕘</span>
          <span class="tile-label">{{ item.name }}</span>
          <span class="tile-sub">{{ item.visits }} visits</span>
        </button>
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
  color: #a6adc8;
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
  color: #6c7086;
}

.link {
  background: none;
  border: none;
  color: #6c7086;
  font-size: 10px;
  font-family: inherit;
  cursor: pointer;
  text-decoration: underline;
}

.link:hover {
  color: #89b4fa;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(148px, 1fr));
  gap: 10px;
}

.tile {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  background: #181825;
  border: 1px solid #313244;
  border-radius: 8px;
  padding: 12px;
  cursor: pointer;
  color: #cdd6f4;
  font-family: inherit;
  text-align: left;
  transition: border-color 0.12s, background 0.12s;
}

.tile:hover {
  background: #242438;
  border-color: #89b4fa;
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
  color: #6c7086;
}

.bar {
  width: 100%;
  height: 4px;
  background: #313244;
  border-radius: 2px;
  overflow: hidden;
  margin-top: 4px;
}

.bar-fill {
  display: block;
  height: 100%;
  background: #89b4fa;
}

.bar-fill.full {
  background: #f38ba8;
}

.remove {
  position: absolute;
  top: 4px;
  right: 4px;
  background: none;
  border: none;
  color: #6c7086;
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
  color: #f38ba8;
}
</style>
