import { OS } from '@/consts'

export const MONACO_FONT_FAMILY =
  '"Cascadia Code NF",' +
  '"Cascadia Code",' +
  'Fira Code,' +
  'JetBrains Mono,' +
  'Roboto Mono,' +
  '"Source Code Pro",' +
  'Consolas,' +
  'Menlo,' +
  'Monaco,' +
  'monospace,' +
  `${OS === 'windows' ? 'twemoji mozilla' : ''}`
