import type { SnapshotDiffHunk } from '@nyanpasu/rpc/types'

// Plain-text search for the YAML and diff viewers. Pure, so it needs no DOM.

/** Where a changed-field chip points: 0-based lines of the field in the YAML. */
export type FieldJump = {
  field: string
  start: number
  end: number
  /** A new value on every click, so choosing the same chip again scrolls again. */
  nonce: number
}

/** The 0-based indexes of the lines that contain `query`, ignoring case. */
export function findLines(lines: readonly string[], query: string): number[] {
  const needle = query.toLowerCase()
  if (!needle) return []
  const matches: number[] = []
  lines.forEach((line, index) => {
    if (line.toLowerCase().includes(needle)) matches.push(index)
  })
  return matches
}

/** The next index in a list of `count`, wrapping at both ends. */
export function cycleIndex(current: number, count: number, delta: 1 | -1) {
  return count === 0 ? 0 : (current + delta + count) % count
}

/** Every line of a diff as plain text, without its sign, in display order. */
export function diffLines(hunks: readonly SnapshotDiffHunk[]): string[] {
  return hunks.flatMap((hunk) => hunk.lines.map((line) => line.slice(1)))
}

const indentOf = (line: string) => line.length - line.trimStart().length

const isBlank = (line: string) => {
  const text = line.trim()
  return text === '' || text.startsWith('#')
}

const escapeRegExp = (text: string) =>
  text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

function isKeyLine(line: string, key: string) {
  return new RegExp(
    `^\\s*(?:"${escapeRegExp(key)}"|'${escapeRegExp(key)}'|${escapeRegExp(key)})\\s*:(?:\\s|$)`,
  ).test(line)
}

/** The last line of the block that starts at `start`. */
function blockEnd(lines: readonly string[], start: number, end: number) {
  const indent = indentOf(lines[start])
  let last = start
  for (let index = start + 1; index <= end; index++) {
    const line = lines[index]
    if (isBlank(line)) continue
    const lineIndent = indentOf(line)
    // A sequence may sit at the same indent as the key that owns it.
    if (lineIndent < indent) break
    if (lineIndent === indent && !line.trimStart().startsWith('- ')) break
    last = index
  }
  return last
}

/**
 * Finds a field in a YAML document by its path, such as `dns.nameserver`. The
 * result is a best effort: a key that is quoted in an unusual way, or that
 * does not appear in the document, gives `null`.
 */
export function locateFieldLine(
  lines: readonly string[],
  field: string,
): { start: number; end: number } | null {
  let from = 0
  let to = lines.length - 1
  let childIndent: number | undefined = 0
  let found: { start: number; end: number } | null = null
  for (const key of field.split('.')) {
    let start = -1
    for (let index = from; index <= to; index++) {
      if (
        !isBlank(lines[index]) &&
        (childIndent === undefined || indentOf(lines[index]) === childIndent) &&
        isKeyLine(lines[index], key)
      ) {
        start = index
        break
      }
    }
    if (start === -1) return null
    const end = blockEnd(lines, start, to)
    found = { start, end }
    from = start + 1
    to = end
    childIndent = lines
      .slice(from, to + 1)
      .find((line) => !isBlank(line))
      ?.search(/\S/)
  }
  return found
}
