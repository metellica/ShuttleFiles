export type TerminalGroup = 'system' | 'visual-studio' | 'git'

export interface TerminalEntry {
  id: string
  label: string
  group: TerminalGroup
}
