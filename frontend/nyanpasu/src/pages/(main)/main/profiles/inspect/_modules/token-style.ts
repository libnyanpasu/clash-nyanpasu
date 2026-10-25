import type { CSSProperties } from 'react'

/** Shiki's HTML style keys are CSS property names; React wants camelCase. */
export function tokenStyle(style: Record<string, string> | undefined) {
  if (!style) return undefined
  return Object.fromEntries(
    Object.entries(style).map(([key, value]) => [
      key.startsWith('--')
        ? key
        : key.replace(/-([a-z])/g, (_, char: string) => char.toUpperCase()),
      value,
    ]),
  ) as CSSProperties
}
