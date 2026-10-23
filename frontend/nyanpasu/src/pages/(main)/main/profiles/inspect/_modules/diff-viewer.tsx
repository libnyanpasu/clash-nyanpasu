import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import type { ThemedToken } from 'shiki'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import type { SnapshotDiffHunk } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { revealWithin, type FindRequest } from './find-bar'
import { diffLines, findLines, type FieldJump } from './text-find'

/** Highlighted tokens for each shown line of a hunk; undefined for markers. */
type HunkTokens = (ThemedToken[] | undefined)[]

/**
 * Each side of a hunk is tokenized as its own contiguous text, so a line is
 * highlighted in the YAML context it has on its side of the change. Only the
 * first `shown` lines are tokenized.
 */
async function tokenizeHunk(
  hunk: SnapshotDiffHunk,
  shown: number,
): Promise<HunkTokens> {
  const { tokenizeYaml } = await import('@/utils/shiki')
  const lines = hunk.lines.slice(0, shown)
  const side = (sign: string) =>
    lines
      .filter((line) => line[0] === ' ' || line[0] === sign)
      .map((line) => line.slice(1))
      .join('\n')
  const [oldTokens, newTokens] = await Promise.all([
    tokenizeYaml(side('-')),
    tokenizeYaml(side('+')),
  ])
  let oldIndex = 0
  let newIndex = 0
  return lines.map((line) => {
    switch (line[0]) {
      case '-':
        return oldTokens[oldIndex++]
      case '+':
        return newTokens[newIndex++]
      case ' ':
        oldIndex++
        return newTokens[newIndex++]
      default:
        return undefined
    }
  })
}

// A transform that touches every proxy yields thousands of hunks; rendering
// and tokenizing them all blocked the page for a long task. Lines are shown
// in pages of this size instead.
const PAGE_LINES = 1000

/** How many lines of each hunk fit in the first `budget` lines. */
function shownLines(hunks: SnapshotDiffHunk[], budget: number) {
  const shown: number[] = []
  for (const hunk of hunks) {
    if (budget <= 0) break
    const lines = Math.min(hunk.lines.length, budget)
    shown.push(lines)
    budget -= lines
  }
  return shown
}

/** Shiki's HTML style keys are CSS property names; React wants camelCase. */
function tokenStyle(style: Record<string, string> | undefined) {
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
  const [budget, setBudget] = useState({ hunks, lines: PAGE_LINES })
  const pagedBudget = budget.hunks === hunks ? budget.lines : PAGE_LINES

  const texts = useMemo(() => diffLines(hunks), [hunks])
  const offsets = useMemo(() => {
    let total = 0
    return hunks.map((hunk) => {
      const start = total
      total += hunk.lines.length
      return start
    })
  }, [hunks])
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

  // The line that was asked for is shown, however many pages that takes.
  const reach = Math.max(activeLine ?? -1, jumpLine ?? -1)
  const lineBudget = Math.max(
    pagedBudget,
    Math.ceil((reach + 1) / PAGE_LINES) * PAGE_LINES,
  )
  const shown = useMemo(
    () => shownLines(hunks, lineBudget),
    [hunks, lineBudget],
  )
  const remaining =
    hunks.reduce((total, hunk) => total + hunk.lines.length, 0) -
    shown.reduce((total, lines) => total + lines, 0)

  // Tokens per hunk with the line count they cover, kept while `hunks` stays.
  const [highlighted, setHighlighted] = useState<{
    hunks: SnapshotDiffHunk[]
    tokens: Map<SnapshotDiffHunk, { shown: number; tokens: HunkTokens }>
  }>()
  const tokens = highlighted?.hunks === hunks ? highlighted.tokens : undefined

  useEffect(() => {
    const pending = shown.flatMap((lines, index) =>
      (tokens?.get(hunks[index])?.shown ?? 0) < lines
        ? [{ hunk: hunks[index], lines }]
        : [],
    )
    if (pending.length === 0) return
    let cancelled = false
    Promise.all(pending.map(({ hunk, lines }) => tokenizeHunk(hunk, lines)))
      .then((result) => {
        if (cancelled) return
        setHighlighted((previous) => {
          const next = new Map(
            previous?.hunks === hunks ? previous.tokens : undefined,
          )
          pending.forEach(({ hunk, lines }, index) =>
            next.set(hunk, { shown: lines, tokens: result[index] }),
          )
          return { hunks, tokens: next }
        })
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [hunks, shown, tokens])

  useEffect(() => {
    const row = regionRef.current?.querySelector<HTMLElement>(
      'tr[data-find-active]',
    )
    if (row) revealWithin(regionRef.current!, row)
  }, [activeLine, find?.query])

  useEffect(() => {
    const row = regionRef.current?.querySelector<HTMLElement>('tr[data-jump]')
    if (row) revealWithin(regionRef.current!, row)
  }, [jump?.nonce])

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
    >
      <table className="min-w-full border-collapse">
        <thead className="sr-only">
          <tr>
            <th>{m.inspect_old_line()}</th>
            <th>{m.inspect_new_line()}</th>
            <th>{m.inspect_change()}</th>
            <th>YAML</th>
          </tr>
        </thead>
        {shown.map((lines, hunkIndex) => {
          const hunk = hunks[hunkIndex]
          const hunkTokens = tokens?.get(hunk)?.tokens
          let oldLine = hunk.old_start
          let newLine = hunk.new_start
          return (
            <tbody key={`${hunk.old_start}-${hunk.new_start}`}>
              <tr className="bg-on-surface/8 text-primary text-[11.5px]">
                <td
                  colSpan={4}
                  className="px-4 py-0.5 whitespace-pre"
                >{`@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@`}</td>
              </tr>
              {hunk.lines.slice(0, lines).map((line, index) => {
                const sign = line[0]
                const oldNumber =
                  sign === '+' || sign === '\\' ? undefined : oldLine++
                const newNumber =
                  sign === '-' || sign === '\\' ? undefined : newLine++
                const flat = offsets[hunkIndex] + index
                const color =
                  sign === '+' ? 'bg-ok/12' : sign === '-' ? 'bg-error/12' : ''
                const markerColor =
                  sign === '+'
                    ? 'text-ok'
                    : sign === '-'
                      ? 'text-error'
                      : 'text-on-surface-variant'
                return (
                  <tr
                    key={index}
                    className={cn(
                      color,
                      'data-find:outline-1 data-find:-outline-offset-1 data-find:outline-yellow-500/50',
                      'data-find-active:outline-2 data-find-active:outline-yellow-500',
                      'data-jump:outline-primary data-jump:outline-2 data-jump:-outline-offset-2',
                    )}
                    data-find={matched.has(flat) ? '' : undefined}
                    data-find-active={flat === activeLine ? '' : undefined}
                    data-jump={flat === jumpLine ? '' : undefined}
                    data-change={
                      sign === '+' ? 'add' : sign === '-' ? 'remove' : 'context'
                    }
                  >
                    <td className="text-tc w-11 min-w-11 pr-2.5 text-right select-none">
                      {oldNumber}
                    </td>
                    <td className="text-tc w-11 min-w-11 pr-2.5 text-right select-none">
                      {newNumber}
                    </td>
                    <td
                      className={cn(
                        'w-[22px] min-w-[22px] text-center select-none',
                        markerColor,
                      )}
                    >
                      {sign}
                    </td>
                    <td className="pr-4 whitespace-pre dark:[&>span]:text-(--shiki-dark)!">
                      {hunkTokens?.[index]?.map((token, tokenIndex) => (
                        <span
                          key={tokenIndex}
                          style={tokenStyle(token.htmlStyle)}
                        >
                          {token.content}
                        </span>
                      )) ?? line.slice(1)}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          )
        })}
      </table>
      {remaining > 0 && (
        <Button
          className="m-2"
          onClick={() => setBudget({ hunks, lines: lineBudget + PAGE_LINES })}
        >
          {m.inspect_diff_show_more({ count: remaining })}
        </Button>
      )}
    </div>
  )
}
