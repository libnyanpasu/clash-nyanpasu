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

/** The columns a line takes in a monospace font; a wide character takes two. */
export function columns(line: string) {
  let count = 0
  for (const char of line) {
    count += char.codePointAt(0)! >= 0x1100 ? 2 : 1
  }
  return count
}

/**
 * The indent of each line, or -1 for a blank or comment line. Found once for a
 * document: every field is looked for in its lines.
 */
function lineIndents(lines: readonly string[]) {
  return Int32Array.from(lines, (line) => {
    const indent = line.search(/\S/)
    return indent === -1 || line[indent] === '#' ? -1 : indent
  })
}

const escapeRegExp = (text: string) =>
  text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Matches a line that holds `key`, bare or quoted. */
function keyLinePattern(key: string) {
  const text = escapeRegExp(key)
  return new RegExp(`^\\s*(?:"${text}"|'${text}'|${text})\\s*:(?:\\s|$)`)
}

/** The last line of the block that starts at `start`. */
function blockEnd(
  lines: readonly string[],
  indents: Int32Array,
  start: number,
  end: number,
) {
  const indent = indents[start]
  let last = start
  for (let index = start + 1; index <= end; index++) {
    const lineIndent = indents[index]
    if (lineIndent === -1) continue
    // A sequence may sit at the same indent as the key that owns it.
    if (lineIndent < indent) break
    if (lineIndent === indent && !lines[index].startsWith('- ', indent)) break
    last = index
  }
  return last
}

/**
 * Finds fields in a YAML document by their paths, such as `dns.nameserver`.
 * The result is a best effort: a key that is quoted in an unusual way, or that
 * does not appear in the document, gives `null`.
 */
export function fieldLocator(lines: readonly string[]) {
  const indents = lineIndents(lines)
  return (field: string): { start: number; end: number } | null => {
    let from = 0
    let to = lines.length - 1
    let childIndent: number | undefined = 0
    let found: { start: number; end: number } | null = null
    for (const key of field.split('.')) {
      // Built once per key: a field is looked for in every line of a document.
      const keyLine = keyLinePattern(key)
      let start = -1
      for (let index = from; index <= to; index++) {
        if (
          indents[index] !== -1 &&
          (childIndent === undefined || indents[index] === childIndent) &&
          keyLine.test(lines[index])
        ) {
          start = index
          break
        }
      }
      if (start === -1) return null
      const end = blockEnd(lines, indents, start, to)
      found = { start, end }
      from = start + 1
      to = end
      childIndent = indents
        .subarray(from, to + 1)
        .find((indent) => indent !== -1)
    }
    return found
  }
}
