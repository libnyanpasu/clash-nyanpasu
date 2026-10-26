import type { CSSProperties } from 'react'
import type { LineToken } from '@/utils/yaml-tokens'

// TextMate's font style bits.
const ITALIC = 1
const BOLD = 2
const UNDERLINE = 4
const STRIKETHROUGH = 8

/** The inline style of a token. */
export function tokenStyle({ color, fontStyle }: LineToken): CSSProperties {
  const lines = [
    fontStyle & UNDERLINE && 'underline',
    fontStyle & STRIKETHROUGH && 'line-through',
  ].filter(Boolean)
  return {
    color,
    fontStyle: fontStyle & ITALIC ? 'italic' : undefined,
    fontWeight: fontStyle & BOLD ? 'bold' : undefined,
    textDecoration: lines.length ? lines.join(' ') : undefined,
  }
}
