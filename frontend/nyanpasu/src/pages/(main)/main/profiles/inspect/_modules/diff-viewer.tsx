import { structuredPatch } from 'diff'
import { useEffect, useState } from 'react'
import { m } from '@/paraglide/messages'

type Patch = NonNullable<ReturnType<typeof structuredPatch>>

export default function DiffViewer({
  before,
  after,
}: {
  before: string
  after: string
}) {
  const [result, setResult] = useState<{
    before: string
    after: string
    patch: Patch
  }>()

  useEffect(() => {
    let cancelled = false
    structuredPatch('before', 'after', before, after, undefined, undefined, {
      context: 3,
      callback: (patch) => {
        if (!cancelled) setResult({ before, after, patch })
      },
    })
    return () => {
      cancelled = true
    }
  }, [before, after])

  if (!result || result.before !== before || result.after !== after) {
    return <p role="status">{m.inspect_diff_loading()}</p>
  }
  if (result.patch.hunks.length === 0) {
    return <p role="status">{m.inspect_unchanged()}</p>
  }

  return (
    <div
      role="region"
      aria-label={m.inspect_diff()}
      tabIndex={0}
      className="bg-surface text-on-surface focus-visible:outline-primary h-[55vh] min-h-64 overflow-auto rounded-lg text-xs focus-visible:outline-2"
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
        {result.patch.hunks.map((hunk) => {
          let oldLine = hunk.oldStart
          let newLine = hunk.newStart
          return (
            <tbody key={`${hunk.oldStart}-${hunk.newStart}`}>
              <tr className="bg-blue-50 text-blue-800 dark:bg-blue-950/50 dark:text-blue-200">
                <td
                  colSpan={4}
                  className="px-3 py-2 whitespace-pre"
                >{`@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`}</td>
              </tr>
              {hunk.lines.map((line, index) => {
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
                    <td className="pr-4 whitespace-pre">{line.slice(1)}</td>
                  </tr>
                )
              })}
            </tbody>
          )
        })}
      </table>
    </div>
  )
}
