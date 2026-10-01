import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { page } from 'vitest/browser'
import { useBreakpoint } from '@nyanpasu/utils'

test('window resizes re-render only when the breakpoint changes', async ({
  onTestFinished,
}) => {
  await page.viewport(1000, 600)

  const seen: string[] = []
  const Consumer = () => {
    const breakpoint = useBreakpoint()
    seen.push(breakpoint)
    return null
  }

  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<Consumer />)
  await expect.poll(() => seen.at(-1)).toBe('md')

  const before = seen.length
  // Dragging a window edge within one breakpoint.
  for (let width = 1010; width <= 1100; width += 10) {
    await page.viewport(width, 600)
  }
  await page.viewport(1300, 600)
  await expect.poll(() => seen.at(-1)).toBe('lg')

  expect(seen.length - before).toBe(1)

  await page.viewport(500, 600)
  await expect.poll(() => seen.at(-1)).toBe('xs')
})
