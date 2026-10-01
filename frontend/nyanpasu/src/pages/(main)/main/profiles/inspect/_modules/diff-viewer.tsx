import { useEffect, useMemo, useState, type CSSProperties } from 'react'
import type { ThemedToken } from 'shiki'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import type { SnapshotDiffHunk } from '@nyanpasu/interface'

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

export default function DiffViewer({ hunks }: { hunks: SnapshotDiffHunk[] }) {
  const [budget, setBudget] = useState({ hunks, lines: PAGE_LINES })
  const lineBudget = budget.hunks === hunks ? budget.lines : PAGE_LINES
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

  if (hunks.length === 0) {
    return <p role="status">{m.inspect_unchanged()}</p>
  }

  return (
    <div
      role="region"
      aria-label={m.inspect_diff()}
      tabIndex={0}
      className="bg-surface text-on-surface focus-visible:outline-primary h-[55vh] min-h-64 overflow-auto rounded-lg text-xs focus-visible:outline-2 @[40rem]:h-auto @[40rem]:min-h-48 @[40rem]:flex-1"
    >
      <table className="min-w-full border-collapse font-mono">
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
              <tr className="bg-blue-50 text-blue-800 dark:bg-blue-950/50 dark:text-blue-200">
                <td
                  colSpan={4}
                  className="px-3 py-2 whitespace-pre"
                >{`@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@`}</td>
              </tr>
              {hunk.lines.slice(0, lines).map((line, index) => {
                const sign = line[0]
                const oldNumber =
                  sign === '+' || sign === '\\' ? undefined : oldLine++
                const newNumber =
                  sign === '-' || sign === '\\' ? undefined : newLine++
                const color =
                  sign === '+'
                    ? 'bg-green-100 text-green-950 dark:bg-green-950/60 dark:text-green-100'
                    : sign === '-'
                      ? 'bg-red-100 text-red-950 dark:bg-red-950/60 dark:text-red-100'
                      : ''
                return (
                  <tr
                    key={index}
                    className={color}
                    data-change={
                      sign === '+' ? 'add' : sign === '-' ? 'remove' : 'context'
                    }
                  >
                    <td className="w-1 min-w-10 px-2 text-right opacity-60 select-none">
                      {oldNumber}
                    </td>
                    <td className="w-1 min-w-10 px-2 text-right opacity-60 select-none">
                      {newNumber}
                    </td>
                    <td className="w-1 px-2 select-none">{sign}</td>
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
