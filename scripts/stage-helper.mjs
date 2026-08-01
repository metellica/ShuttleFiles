// Copies the freshly built shell-menu helper into src-tauri/binaries/,
// which tauri.conf.json declares as a bundled resource.
//
// The helper cannot be referenced straight out of target/release: the
// Tauri build validates resource paths while compiling the *debug*
// crate too, so the path has to exist in every profile.

import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const name = process.platform === 'win32' ? 'shellmenu.exe' : 'shellmenu'

const source = join(root, 'src-tauri', 'target', 'release', name)
const targetDir = join(root, 'src-tauri', 'binaries')
const target = join(targetDir, name)

if (!existsSync(source)) {
  console.error(`stage-helper: ${source} not found - build it first`)
  process.exit(1)
}

mkdirSync(targetDir, { recursive: true })
copyFileSync(source, target)
console.log(`stage-helper: ${target}`)
