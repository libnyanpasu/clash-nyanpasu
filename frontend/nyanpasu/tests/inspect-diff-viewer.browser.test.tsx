import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { m } from '@/paraglide/messages'
import type { SnapshotDiffHunk } from '@nyanpasu/rpc/types'
import DiffViewer from '../src/pages/(main)/main/profiles/inspect/_modules/diff-viewer'

const hunk = (start: number, lines: number): SnapshotDiffHunk => ({
  old_start: start,
  old_lines: lines,
  new_start: start,
  new_lines: lines,
  lines: Array.from({ length: lines }, (_, i) => ` key-${start + i}: value`),
})

test('a large diff shows its lines a page at a time', async ({
  onTestFinished,
}) => {
  // 600 + 900 lines: the first page ends inside the second hunk.
  const hunks = [hunk(1, 600), hunk(1001, 900)]
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<DiffViewer hunks={hunks} />)

  const rows = () => container.querySelectorAll('tr[data-change]')
  await expect.poll(() => rows().length).toBe(1000)
  // The cut hunk keeps its own numbering.
  expect(rows()[999].textContent).toContain('key-1400')
  expect(rows()[999].querySelector('td')?.textContent).toBe('1400')

  const more = [...container.querySelectorAll('button')].find(
    (button) => button.textContent === m.inspect_diff_show_more({ count: 500 }),
  )
  expect(more).toBeDefined()
  more!.click()
  await expect.poll(() => rows().length).toBe(1500)
  expect(container.querySelector('button')).toBeNull()
})
