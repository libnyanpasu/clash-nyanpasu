import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ComponentProps } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import type { SnapshotDiffHunk } from '@nyanpasu/rpc/types'
import DiffViewer from '../src/pages/(main)/main/profiles/inspect/_modules/diff-viewer'

const hunk = (start: number, lines: number): SnapshotDiffHunk => ({
  old_start: start,
  old_lines: lines,
  new_start: start,
  new_lines: lines,
  lines: Array.from({ length: lines }, (_, i) => ` key-${start + i}: value`),
})

const renderViewer = async (
  props: Partial<ComponentProps<typeof DiffViewer>> & {
    hunks: SnapshotDiffHunk[]
  },
  onTestFinished: (callback: () => void) => void,
) => {
  const container = document.createElement('div')
  // A bounded height makes the region scroll, as it does in the page.
  container.style.cssText =
    'display:flex;flex-direction:column;height:300px;width:600px'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<DiffViewer {...props} />)
  const region = await vi.waitUntil(() =>
    container.querySelector<HTMLElement>('[role="region"]'),
  )
  return { container, root, region }
}

const rowsOf = (container: HTMLElement) =>
  container.querySelectorAll('[data-change]')

test('a large diff mounts only the lines in view', async ({
  onTestFinished,
}) => {
  // 600 + 900 lines, each hunk under its own header.
  const hunks = [hunk(1, 600), hunk(1001, 900)]
  const { container, region } = await renderViewer({ hunks }, onTestFinished)

  await expect.poll(() => rowsOf(container).length).toBeGreaterThan(0)
  expect(rowsOf(container).length).toBeLessThan(100)
  // Every row keeps its height, so the region scrolls over the whole diff.
  expect(region.scrollHeight).toBe((2 + 1500) * 20)

  region.scrollTop = region.scrollHeight
  // The last line keeps its hunk's numbering.
  await expect
    .poll(() => [...rowsOf(container)].at(-1)?.textContent)
    .toContain('key-1900')
  expect(
    [...rowsOf(container)].at(-1)?.querySelector('[role="cell"]')?.textContent,
  ).toBe('1900')
})

const inside = (region: HTMLElement, row: Element) => {
  const box = region.getBoundingClientRect()
  const rect = row.getBoundingClientRect()
  return rect.top >= box.top - 1 && rect.bottom <= box.bottom + 1
}

test('find marks every matching line and keeps the change rows', async ({
  onTestFinished,
}) => {
  const hunks = [hunk(1, 20)]
  const { container } = await renderViewer(
    { hunks, find: { query: 'key-1', active: 2 } },
    onTestFinished,
  )

  // key-1 and key-10 .. key-19.
  await expect
    .poll(() => container.querySelectorAll('[data-find]').length)
    .toBe(11)
  expect(rowsOf(container)).toHaveLength(20)
  const active = container.querySelectorAll('[data-find-active]')
  expect(active).toHaveLength(1)
  expect(active[0].textContent).toContain('key-11')
})

test('find scrolls to a match far down a large diff', async ({
  onTestFinished,
}) => {
  const hunks = [hunk(1, 600), hunk(1001, 900)]
  const { container, region } = await renderViewer(
    { hunks, find: { query: 'key-1500:', active: 0 } },
    onTestFinished,
  )

  const active = await vi.waitUntil(() =>
    container.querySelector('[data-find-active]'),
  )
  expect(active.textContent).toContain('key-1500')
  expect(region.scrollTop).toBeGreaterThan(0)
  await expect.poll(() => inside(region, active)).toBe(true)
})

test('find moves to another match', async ({ onTestFinished }) => {
  const hunks = [hunk(1, 300)]
  const { container, root, region } = await renderViewer(
    { hunks, find: { query: 'key-250:', active: 0 } },
    onTestFinished,
  )
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)

  root.render(
    <DiffViewer hunks={hunks} find={{ query: 'key-3:', active: 0 }} />,
  )
  await expect
    .poll(() => container.querySelector('[data-find-active]')?.textContent)
    .toContain('key-3:')
  await expect.poll(() => region.scrollTop).toBe(0)
})

test('a jump marks the first kept line of a field and scrolls to it', async ({
  onTestFinished,
}) => {
  const hunks = [hunk(1, 600), hunk(1001, 900)]
  const { container, region } = await renderViewer(
    { hunks, jump: { field: 'f', start: 1500, end: 1510, nonce: 1 } },
    onTestFinished,
  )

  const row = await vi.waitUntil(() => container.querySelector('[data-jump]'))
  // New line 1501 is the 501st line of the second hunk.
  expect(row.textContent).toContain('key-1501')
  await expect.poll(() => inside(region, row)).toBe(true)
})

test('a jump scrolls again for a new request and prefers kept lines', async ({
  onTestFinished,
}) => {
  const hunks: SnapshotDiffHunk[] = [
    {
      old_start: 5,
      old_lines: 2,
      new_start: 5,
      new_lines: 1,
      lines: ['-gone: 1', ' kept: 2'],
    },
    hunk(100, 200),
  ]
  const { container, root, region } = await renderViewer(
    { hunks, jump: { field: 'f', start: 250, end: 260, nonce: 1 } },
    onTestFinished,
  )
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)

  region.scrollTop = 0
  root.render(
    <DiffViewer
      hunks={hunks}
      jump={{ field: 'f', start: 4, end: 4, nonce: 2 }}
    />,
  )
  // Both lines sit at new line 5, and the removed one only counts alone.
  await expect
    .poll(() => container.querySelector('[data-jump]')?.textContent)
    .toContain('kept: 2')

  root.render(
    <DiffViewer
      hunks={hunks}
      jump={{ field: 'f', start: 250, end: 260, nonce: 3 }}
    />,
  )
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
})

test('a field with only removed lines jumps to the removed line', async ({
  onTestFinished,
}) => {
  const hunks: SnapshotDiffHunk[] = [
    {
      old_start: 1,
      old_lines: 2,
      new_start: 1,
      new_lines: 0,
      lines: ['-a: 1', '-b: 2'],
    },
  ]
  const { container } = await renderViewer(
    { hunks, jump: { field: 'a', start: 0, end: 0, nonce: 1 } },
    onTestFinished,
  )
  await expect
    .poll(() => container.querySelector('[data-jump]')?.textContent)
    .toContain('a: 1')
})

test('without find or jump nothing is marked', async ({ onTestFinished }) => {
  const { container } = await renderViewer(
    { hunks: [hunk(1, 5)] },
    onTestFinished,
  )
  await expect.poll(() => rowsOf(container).length).toBe(5)
  expect(container.querySelector('[data-find],[data-jump]')).toBeNull()
})

test('each side of a change is highlighted', async ({ onTestFinished }) => {
  const { container } = await renderViewer(
    {
      hunks: [
        {
          old_start: 1,
          old_lines: 1,
          new_start: 1,
          new_lines: 1,
          lines: ['-mode: rule', '+mode: global'],
        },
      ],
    },
    onTestFinished,
  )
  // The first document highlighted in a run also loads the highlighter.
  await vi.waitUntil(
    () => {
      const rows = [...rowsOf(container)]
      return (
        rows.length === 2 &&
        rows.every((row) =>
          row.querySelector('[role="cell"] > span[style*="color"]'),
        )
      )
    },
    { timeout: 5000 },
  )
})
