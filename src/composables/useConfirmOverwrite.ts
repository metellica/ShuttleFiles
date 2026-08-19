import { createApp, h, ref } from 'vue'
import type { FileEntry } from '@/types/filesystem'

export type OverwriteChoice = 'overwrite' | 'skip' | 'cancel'

/** Lightweight meta shown in the dialog for both the incoming (source)
 *  and existing (destination) side of a conflict. */
export interface OverwriteMeta {
  name: string
  size: number
  modified: number
  isDir: boolean
}

/** One name that exists both in the incoming paste/drop and at the
 *  destination. `source` is fetched best-effort — undefined while still
 *  loading, null if it couldn't be determined (e.g. removed mid-flight). */
export interface OverwriteConflict {
  name: string
  dest: OverwriteMeta
  source?: OverwriteMeta | null
}

export function toMeta(e: FileEntry): OverwriteMeta {
  return { name: e.name, size: e.size, modified: e.modified, isDir: e.isDir }
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '-'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let size = bytes
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024
    i++
  }
  return `${size.toFixed(i === 0 ? 0 : 1)} ${units[i]}`
}

function formatDate(ts: number): string {
  if (!ts) return '-'
  return new Date(ts * 1000).toLocaleString()
}

function metaRow(label: string, meta: OverwriteMeta | null | undefined) {
  if (meta === undefined) {
    return h('div', { class: 'sp-ow-meta-block' }, [
      h('div', { class: 'sp-ow-meta-title' }, label),
      h('div', { class: 'sp-ow-meta-line sp-ow-meta-loading' }, 'Loading…'),
    ])
  }
  if (meta === null) {
    return h('div', { class: 'sp-ow-meta-block' }, [
      h('div', { class: 'sp-ow-meta-title' }, label),
      h('div', { class: 'sp-ow-meta-line sp-ow-meta-loading' }, 'Unavailable'),
    ])
  }
  return h('div', { class: 'sp-ow-meta-block' }, [
    h('div', { class: 'sp-ow-meta-title' }, label),
    h('div', { class: 'sp-ow-meta-line' }, `Size: ${meta.isDir ? '-' : formatSize(meta.size)}`),
    h('div', { class: 'sp-ow-meta-line' }, `Modified: ${formatDate(meta.modified)}`),
  ])
}

/**
 * Asks whether to proceed when pasting/moving files that already exist
 * at the destination. Rows are compact by default (name only); clicking
 * the info button on a row expands it to show both the source (incoming)
 * and destination (existing) file's size/modified time.
 *
 * Resolves 'overwrite' (replace all conflicts), 'skip' (proceed with only
 * the non-conflicting files), or 'cancel' (abort the whole paste/move).
 */
export function confirmOverwrite(conflicts: OverwriteConflict[]): Promise<OverwriteChoice> {
  return new Promise((resolve) => {
    const host = document.createElement('div')
    document.body.appendChild(host)

    const app = createApp({
      setup() {
        const expanded = ref<Set<string>>(new Set())

        function toggle(name: string) {
          const next = new Set(expanded.value)
          if (next.has(name)) next.delete(name)
          else next.add(name)
          expanded.value = next
        }

        function done(result: OverwriteChoice) {
          app.unmount()
          host.remove()
          resolve(result)
        }

        return () =>
          h(
            'div',
            {
              class: 'sp-overlay',
              onClick: (e: MouseEvent) => {
                if (e.target === e.currentTarget) done('cancel')
              },
              onKeydown: (e: KeyboardEvent) => {
                if (e.key === 'Escape') done('cancel')
              },
            },
            [
              h('div', { class: 'sp-dialog sp-overwrite-dialog' }, [
                h(
                  'div',
                  { class: 'sp-title' },
                  conflicts.length === 1
                    ? '1 item already exists at the destination'
                    : `${conflicts.length} items already exist at the destination`
                ),
                h(
                  'div',
                  { class: 'sp-overwrite-list' },
                  conflicts.map((c) => {
                    const isOpen = expanded.value.has(c.name)
                    return h('div', { class: 'sp-overwrite-item' }, [
                      h('div', { class: 'sp-overwrite-row' }, [
                        h('span', { class: 'sp-ow-name', title: c.name }, c.name),
                        h(
                          'button',
                          {
                            class: 'sp-ow-info-btn',
                            title: isOpen ? 'Hide details' : 'Show details',
                            'aria-label': 'Toggle file details',
                            onClick: () => toggle(c.name),
                          },
                          'i'
                        ),
                      ]),
                      isOpen
                        ? h('div', { class: 'sp-ow-meta' }, [
                            metaRow('Source', c.source),
                            metaRow('Destination', c.dest),
                          ])
                        : null,
                    ])
                  })
                ),
                h('div', { class: 'sp-actions' }, [
                  h(
                    'button',
                    { class: 'sp-btn sp-cancel', onClick: () => done('cancel') },
                    'Cancel'
                  ),
                  h(
                    'button',
                    { class: 'sp-btn sp-cancel', onClick: () => done('skip') },
                    'Skip existing'
                  ),
                  h(
                    'button',
                    { class: 'sp-btn sp-ok', onClick: () => done('overwrite') },
                    'Overwrite'
                  ),
                ]),
              ]),
            ]
          )
      },
    })

    app.mount(host)
  })
}

// Styles for the dialog shell plus the overwrite list. ShuttleFiles'
// own prompt (usePrompt.ts) renders through a Vue SFC rather than this
// hand-built-`h()` pattern, so the shared .sp-overlay/.sp-dialog/
// .sp-actions/.sp-btn base isn't defined anywhere else — inject it here.
const STYLE_ID = 'sp-overwrite-style'
if (!document.getElementById(STYLE_ID)) {
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = `
.sp-overlay {
  position: fixed;
  inset: 0;
  background: var(--overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.sp-dialog {
  background: var(--bg-primary);
  border: 1px solid var(--text-disabled);
  border-radius: 8px;
  padding: 20px;
  width: 380px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.sp-title {
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 600;
  word-break: break-all;
}
.sp-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.sp-btn {
  padding: 5px 14px;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  border: 1px solid var(--text-disabled);
}
.sp-cancel {
  background: var(--surface);
  color: var(--text-primary);
}
.sp-ok {
  background: var(--accent);
  color: var(--accent-text);
  border-color: var(--accent);
}
.sp-ok:hover {
  background: var(--accent-hover);
}
.sp-overwrite-dialog {
  width: 460px;
}
.sp-overwrite-list {
  max-height: 280px;
  overflow-y: auto;
  border: 1px solid var(--text-disabled);
  border-radius: 4px;
}
.sp-overwrite-item {
  border-bottom: 1px solid var(--text-disabled);
}
.sp-overwrite-item:last-child {
  border-bottom: none;
}
.sp-overwrite-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  font-size: 12px;
  color: var(--text-primary);
}
.sp-ow-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sp-ow-info-btn {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1px solid var(--text-secondary);
  background: transparent;
  color: var(--text-secondary);
  font-size: 11px;
  font-style: italic;
  line-height: 1;
  cursor: pointer;
  padding: 0;
}
.sp-ow-info-btn:hover {
  color: var(--text-primary);
  border-color: var(--text-primary);
}
.sp-ow-meta {
  display: flex;
  gap: 16px;
  padding: 0 10px 8px 10px;
}
.sp-ow-meta-block {
  flex: 1;
  min-width: 0;
}
.sp-ow-meta-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 2px;
}
.sp-ow-meta-line {
  font-size: 11px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.sp-ow-meta-loading {
  color: var(--text-secondary);
  font-style: italic;
}
`
  document.head.appendChild(style)
}
