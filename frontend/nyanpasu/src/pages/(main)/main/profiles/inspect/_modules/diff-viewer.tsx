import { m } from '@/paraglide/messages'
import type { SnapshotDiffHunk } from '@nyanpasu/interface'

export default function DiffViewer({ hunks }: { hunks: SnapshotDiffHunk[] }) {
  if (hunks.length === 0) {
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
        {hunks.map((hunk) => {
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
