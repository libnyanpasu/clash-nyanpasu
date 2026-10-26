import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import { useColorScheme, type ColorScheme } from '@/hooks/use-color-scheme'
import { m } from '@/paraglide/messages'
import {
  lineTokens,
  MAX_HIGHLIGHT_LINES,
  tokenizeYaml,
  type DocumentTokens,
} from '@/utils/yaml-tokens'
import type { SnapshotDiffHunk } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FindRequest } from './find-bar'
import { columns, diffLines, findLines, type FieldJump } from './text-find'
import { tokenStyle } from './token-style'

// Every row, a hunk's header or one of its lines, has the same height, so a
// row sits at a fixed offset and only the rows in view are mounted. A table
// of a thousand rows took a layout of 250 ms in WebKit.
const ROW_HEIGHT = 20
// Rows mounted beyond each edge of the view, so scrolling shows no gap.
const OVERSCAN = 20
// Each line number column fits the longest number, `--number` wide, with this
// much room around it; then come the change sign and, after the longest line,
// some room.
const NUMBER_PADDING = 16 + 12
const SIGN_WIDTH = 22
const END_PADDING = 16

const rowClassName =
  'absolute inset-x-0 grid h-5 grid-cols-[var(--number)_var(--number)_22px_1fr]'
const numberClassName = 'text-tc pr-3 pl-4 text-right select-none'

/**
 * Where a line of a hunk is in the tokens of its side; undefined for markers.
 */
type LinePlace = { side: DocumentTokens; line: number } | undefined

/** The two sides of a hunk, as contiguous texts. */
function hunkSides(hunk: SnapshotDiffHunk) {
  const side = (sign: string) =>
    hunk.lines
      .filter((line) => line[0] === ' ' || line[0] === sign)
      .map((line) => line.slice(1))
      .join('\n')
  return [side('-'), side('+')]
}

/** Each line of a hunk with the tokens of the side it belongs to. */
function hunkPlaces(
  hunk: SnapshotDiffHunk,
  oldTokens: DocumentTokens,
  newTokens: DocumentTokens,
): LinePlace[] {
  let oldIndex = 0
  let newIndex = 0
  return hunk.lines.map((line) => {
    switch (line[0]) {
      case '-':
        return { side: oldTokens, line: oldIndex++ }
      case '+':
        return { side: newTokens, line: newIndex++ }
      case ' ':
        oldIndex++
        return { side: newTokens, line: newIndex++ }
      default:
        return undefined
    }
  })
}

/**
 * Each side of a hunk is tokenized as its own contiguous text, so a line is
 * highlighted in the YAML context it has on its side of the change. The lines
 * of every hunk are placed in one list, in display order.
 */
async function tokenizeHunks(
  hunks: SnapshotDiffHunk[],
  scheme: ColorScheme,
): Promise<LinePlace[]> {
  const documents = await tokenizeYaml(hunks.flatMap(hunkSides), scheme)
  return hunks.flatMap((hunk, index) =>
    hunkPlaces(hunk, documents[index * 2], documents[index * 2 + 1]),
  )
}

/** A line of a hunk, highlighted once its side has been tokenized. */
function lineContent(place: LinePlace, text: string) {
  if (!place) return text
  return lineTokens(place.side, place.line, text).map((token, index) => (
    <span key={index} style={tokenStyle(token)}>
      {token.text}
    </span>
  ))
}

/** A row of the diff: a hunk's header, or the line at `flat` among them all. */
type DiffRow =
  | { hunk: SnapshotDiffHunk }
  | {
      hunk: SnapshotDiffHunk
      flat: number
      text: string
      sign: string
      oldNumber?: number
      newNumber?: number
    }

/** The rows of the hunks, and the row of each line. */
function diffRows(hunks: SnapshotDiffHunk[]) {
  const rows: DiffRow[] = []
  const lineRows: number[] = []
  for (const hunk of hunks) {
    rows.push({ hunk })
    let oldLine = hunk.old_start
    let newLine = hunk.new_start
    for (const line of hunk.lines) {
      const sign = line[0]
      lineRows.push(rows.length)
      rows.push({
        hunk,
        flat: lineRows.length - 1,
        text: line.slice(1),
        sign,
        oldNumber: sign === '+' || sign === '\\' ? undefined : oldLine++,
        newNumber: sign === '-' || sign === '\\' ? undefined : newLine++,
      })
    }
  }
  return { rows, lineRows }
}

/**
 * The position, among all the lines of the hunks, of the first line of a
 * field. A removed line has no place in the new text, so it only counts when
 * no kept line is in the field.
 */
function jumpIndex(hunks: SnapshotDiffHunk[], jump: FieldJump) {
  let flat = 0
  let removed: number | undefined
  for (const hunk of hunks) {
    let newLine = hunk.new_start
    for (const line of hunk.lines) {
      const position = newLine - 1
      if (position >= jump.start && position <= jump.end) {
        if (line[0] !== '-') return flat
        removed ??= flat
      }
      if (line[0] !== '-' && line[0] !== '\\') newLine++
      flat++
    }
  }
  return removed
}

/** Scrolls a row to the middle of the region, unless it is already in view. */
function revealRow(region: HTMLElement, row: number) {
  const top = row * ROW_HEIGHT
  const { scrollTop, clientHeight } = region
  if (top >= scrollTop && top + ROW_HEIGHT <= scrollTop + clientHeight) return
  region.scrollTop = top - (clientHeight - ROW_HEIGHT) / 2
}

export default function DiffViewer({
  hunks,
  find,
  jump,
}: {
  hunks: SnapshotDiffHunk[]
  /** Marks the lines that contain the text and scrolls to the current one. */
  find?: FindRequest
  /** Scrolls to the first line of a field and marks it. */
  jump?: FieldJump
}) {
  const regionRef = useRef<HTMLDivElement>(null)

  const texts = useMemo(() => diffLines(hunks), [hunks])
  const { rows, lineRows } = useMemo(() => diffRows(hunks), [hunks])
  const lastNumber = useMemo(
    () =>
      hunks.reduce(
        (last, hunk) =>
          Math.max(
            last,
            hunk.old_start + hunk.old_lines,
            hunk.new_start + hunk.new_lines,
          ),
        0,
      ),
    [hunks],
  )
  // The rows are absolutely placed, so the longest line sets the width.
  const width = useMemo(
    () => texts.reduce((widest, text) => Math.max(widest, columns(text)), 0),
    [texts],
  )
  const matches = useMemo(
    () => (find?.query ? findLines(texts, find.query) : []),
    [texts, find?.query],
  )
  const matched = useMemo(() => new Set(matches), [matches])
  const activeLine = find ? matches[find.active] : undefined
  const jumpLine = useMemo(
    () => (jump ? jumpIndex(hunks, jump) : undefined),
    [hunks, jump],
  )

  // The tokens of every line, kept while `hunks` and the color scheme stay.
  const scheme = useColorScheme()
  const [highlighted, setHighlighted] = useState<{
    hunks: SnapshotDiffHunk[]
    scheme: ColorScheme
    places: LinePlace[]
  }>()
  const places =
    highlighted?.hunks === hunks && highlighted.scheme === scheme
      ? highlighted.places
      : undefined

  useEffect(() => {
    if (texts.length > MAX_HIGHLIGHT_LINES) return
    let cancelled = false
    tokenizeHunks(hunks, scheme)
      .then((result) => {
        if (!cancelled) setHighlighted({ hunks, scheme, places: result })
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [hunks, scheme, texts])

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => regionRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: OVERSCAN,
  })

  useEffect(() => {
    if (activeLine !== undefined) {
      revealRow(regionRef.current!, lineRows[activeLine])
    }
  }, [activeLine, find?.query, lineRows])

  // The same field chosen again scrolls again: its nonce is new.
  useEffect(() => {
    if (jumpLine !== undefined)
      revealRow(regionRef.current!, lineRows[jumpLine])
  }, [jump?.nonce, jumpLine, lineRows])

  if (hunks.length === 0) {
    return (
      <p role="status" className="text-on-surface-variant p-6 text-[13px]">
        {m.inspect_unchanged()}
      </p>
    )
  }

  return (
    <div
      ref={regionRef}
      role="region"
      aria-label={m.inspect_diff()}
      tabIndex={0}
      className="bg-code text-on-surface focus-visible:outline-primary min-h-0 flex-1 overflow-auto font-mono text-[12.5px] leading-5 focus-visible:outline-2 focus-visible:-outline-offset-2"
      style={
        {
          '--number': `calc(${String(lastNumber).length}ch + ${NUMBER_PADDING}px)`,
        } as CSSProperties
      }
    >
      <div
        role="table"
        aria-rowcount={rows.length + 1}
        data-highlighted={places ? '' : undefined}
        className="relative"
        style={{
          height: virtualizer.getTotalSize(),
          minWidth: `calc(var(--number) * 2 + ${SIGN_WIDTH + END_PADDING}px + ${width}ch)`,
        }}
      >
        <div role="row" aria-rowindex={1} className="sr-only">
          <span role="columnheader">{m.inspect_old_line()}</span>
          <span role="columnheader">{m.inspect_new_line()}</span>
          <span role="columnheader">{m.inspect_change()}</span>
          <span role="columnheader">YAML</span>
        </div>
        {virtualizer.getVirtualItems().map(({ index, start }) => {
          const row = rows[index]
          if (!('flat' in row)) {
            const { hunk } = row
            return (
              <div
                key={index}
                role="row"
                aria-rowindex={index + 2}
                className={cn(
                  rowClassName,
                  'bg-on-surface/8 text-primary text-[11.5px]',
                )}
                style={{ top: start }}
              >
                <span
                  role="cell"
                  aria-colspan={4}
                  className="col-span-4 px-4 whitespace-pre"
                >{`@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@`}</span>
              </div>
            )
          }
          const { flat, sign } = row
          return (
            <div
              key={index}
              role="row"
              aria-rowindex={index + 2}
              className={cn(
                rowClassName,
                sign === '+' && 'bg-ok/12',
                sign === '-' && 'bg-error/12',
                'data-find:outline-1 data-find:-outline-offset-1 data-find:outline-yellow-500/50',
                // Drawn over the rows around it, which come later.
                'data-find-active:z-10 data-find-active:outline-2 data-find-active:outline-yellow-500',
                'data-jump:outline-primary data-jump:outline-2 data-jump:-outline-offset-2',
              )}
              style={{ top: start }}
              data-find={matched.has(flat) ? '' : undefined}
              data-find-active={flat === activeLine ? '' : undefined}
              data-jump={flat === jumpLine ? '' : undefined}
              data-change={
                sign === '+' ? 'add' : sign === '-' ? 'remove' : 'context'
              }
            >
              <span role="cell" className={numberClassName}>
                {row.oldNumber}
              </span>
              <span role="cell" className={numberClassName}>
                {row.newNumber}
              </span>
              <span
                role="cell"
                className={cn(
                  'text-center select-none',
                  sign === '+'
                    ? 'text-ok'
                    : sign === '-'
                      ? 'text-error'
                      : 'text-on-surface-variant',
                )}
              >
                {sign}
              </span>
              <span role="cell" className="pr-4 whitespace-pre">
                {lineContent(places?.[flat], row.text)}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
