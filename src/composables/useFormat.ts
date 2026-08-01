/** Human-readable byte size, matching Explorer's 1024-based units. */
export function formatSize(bytes: number): string {
  if (bytes <= 0) return ''
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  return `${unit === 0 ? value : value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`
}

/** Unix seconds -> locale date/time. Returns '' for unknown (0). */
export function formatTime(unixSeconds: number): string {
  if (!unixSeconds) return ''
  const d = new Date(unixSeconds * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

const ICONS: Record<string, string> = {
  // archives
  zip: '🗜', '7z': '🗜', rar: '🗜', gz: '🗜', tar: '🗜', xz: '🗜', bz2: '🗜',
  // images
  png: '🖼', jpg: '🖼', jpeg: '🖼', gif: '🖼', bmp: '🖼', svg: '🖼', webp: '🖼', ico: '🖼',
  // media
  mp3: '🎵', wav: '🎵', flac: '🎵', ogg: '🎵', m4a: '🎵',
  mp4: '🎬', mkv: '🎬', avi: '🎬', mov: '🎬', webm: '🎬',
  // docs
  pdf: '📕', doc: '📘', docx: '📘', xls: '📗', xlsx: '📗', ppt: '📙', pptx: '📙',
  txt: '📄', md: '📝', log: '📄', csv: '📊',
  // code
  ts: '📜', js: '📜', json: '📜', html: '📜', css: '📜', vue: '📜',
  rs: '📜', py: '📜', go: '📜', java: '📜', c: '📜', cpp: '📜', h: '📜',
  yml: '📜', yaml: '📜', toml: '📜', xml: '📜', sh: '📜', ps1: '📜',
  // executables
  exe: '⚙', msi: '⚙', dll: '⚙', bat: '⚙',
}

export function fileIcon(ext: string, isDir: boolean): string {
  if (isDir) return '📁'
  return ICONS[ext] ?? '📄'
}

const DRIVE_ICONS: Record<string, string> = {
  fixed: '💽',
  removable: '💾',
  network: '🌐',
  cdrom: '💿',
  ramdisk: '⚡',
  unknown: '💽',
}

export function driveIcon(kind: string): string {
  return DRIVE_ICONS[kind] ?? '💽'
}
