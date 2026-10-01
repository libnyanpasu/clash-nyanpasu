import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { Sparkline } from '@/components/ui/sparkline'

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

const series = (start: number) =>
  Array.from({ length: 10 }, (_, i) => ((start + i) * 37) % 100)

// Browser tests load no Tailwind; these are the utilities the chart uses.
const STYLES = `
  .size-full { width: 100%; height: 100% }
  .h-full { height: 100% }
  .overflow-hidden { overflow: hidden }
  .overflow-visible { overflow: visible }
`

function mount(onTestFinished: (fn: () => void) => void) {
  const style = document.createElement('style')
  style.textContent = STYLES
  document.head.append(style)
  const container = document.createElement('div')
  container.style.cssText = 'width: 300px; height: 100px; display: flex'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })

  // The dashboard passes a freshly built array on every render.
  const controls = { push: () => {}, rerender: () => {} }
  const Owner = () => {
    const [start, setStart] = useState(0)
    const [, setTick] = useState(0)
    controls.push = () => setStart((value) => value + 1)
    controls.rerender = () => setTick((tick) => tick + 1)
    return <Sparkline data={series(start)} animationDuration={0.4} />
  }
  root.render(<Owner />)
  return { container, controls }
}

const linePath = (container: HTMLElement) =>
  container.querySelector('path.line')?.getAttribute('d')

test('re-renders with unchanged data do not read layout', async ({
  onTestFinished,
}) => {
  const { container, controls } = mount(onTestFinished)
  await expect.poll(() => linePath(container)).toBeTruthy()

  const measure = vi.spyOn(Element.prototype, 'getBoundingClientRect')
  onTestFinished(() => measure.mockRestore())
  for (let i = 0; i < 5; i++) {
    flushSync(() => controls.rerender())
    await nextFrame()
  }
  expect(measure).not.toHaveBeenCalled()
})

test('a new sample scrolls without per-frame DOM writes', async ({
  onTestFinished,
}) => {
  const { container, controls } = mount(onTestFinished)
  await expect.poll(() => linePath(container)).toBeTruthy()
  await nextFrame()

  let writes = 0
  const observer = new MutationObserver((records) => (writes += records.length))
  observer.observe(container, { attributes: true, subtree: true })
  onTestFinished(() => observer.disconnect())

  flushSync(() => controls.push())
  await sleep(200)
  // Mid-scroll: a second sample continues from where the first one is.
  flushSync(() => controls.push())
  await sleep(600)

  // Each sample redraws the paths once and, when the scroll ends, swaps in
  // the settled paths once.
  expect(writes).toBeLessThan(20)
  await expect
    .poll(() =>
      container.querySelector('svg')!.getAnimations({ subtree: true }),
    )
    .toHaveLength(0)
})

test('the chart follows a resize without waiting for a sample', async ({
  onTestFinished,
}) => {
  const { container } = mount(onTestFinished)
  await expect.poll(() => linePath(container)).toBeTruthy()
  const before = linePath(container)

  container.style.width = '600px'
  await expect.poll(() => linePath(container)).not.toBe(before)
})
