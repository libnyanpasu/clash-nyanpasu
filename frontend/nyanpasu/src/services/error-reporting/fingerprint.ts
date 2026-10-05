import { stripQuery, type EventDraft } from './normalize'

/** Replaces values that differ between repeats of the same problem. */
export function normalizeMessage(message: string): string {
  return message
    .replace(
      /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi,
      '<uuid>',
    )
    .replace(/\b(?:0x)?[0-9a-f]{8,}\b/gi, '<hex>')
    .replace(/\d+/g, '<n>')
}

/** The first stack line naming a code location, without query strings. */
export function firstFrame(stack: string | null): string {
  if (!stack) return ''

  const frame = stack
    .split('\n')
    .map((line) => line.trim())
    .find((line) => line.startsWith('at ') || line.includes('@'))
  return frame ? stripQuery(frame.replace(/\?[^:)\s]*/g, '')) : ''
}

/** cyrb53: a fast 53-bit string hash, rendered as hex. */
function hash(text: string): string {
  let h1 = 0xdeadbeef
  let h2 = 0x41c6ce57
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i)
    h1 = Math.imul(h1 ^ ch, 2654435761)
    h2 = Math.imul(h2 ^ ch, 1597334677)
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507)
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909)
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507)
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909)
  return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(16)
}

/** Groups repeats of one problem: `<kind>-<hash>`, within `[0-9a-z_-]`. */
export function fingerprint(draft: EventDraft): string {
  const key = [
    draft.kind,
    draft.error_name ?? '',
    normalizeMessage(draft.message),
    firstFrame(draft.stack),
  ].join('\u0000')
  return `${draft.kind}-${hash(key)}`
}
