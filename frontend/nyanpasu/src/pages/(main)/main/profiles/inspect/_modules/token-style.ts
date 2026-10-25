import type { CSSProperties } from 'react'
import type { ThemedToken } from 'shiki'

// TextMate's font style bits.
const ITALIC = 1
const BOLD = 2
const UNDERLINE = 4
const STRIKETHROUGH = 8

/** The inline style of a token tokenized in one theme. */
export function tokenStyle({
  color,
  fontStyle = 0,
}: Pick<ThemedToken, 'color' | 'fontStyle'>): CSSProperties {
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
