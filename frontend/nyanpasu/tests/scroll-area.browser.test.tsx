import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { ScrollArea, useScrollArea } from '@/components/ui/scroll-area'

const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

function mount(onTestFinished: (fn: () => void) => void) {
  const renders = { count: 0 }
  const seen: ReturnType<typeof useScrollArea>[] = []

  const Consumer = () => {
    const state = useScrollArea()
    renders.count += 1
    seen.push(state)
    return <div style={{ height: 4000 }} />
  }

  // Browser tests load no Tailwind, so give the viewport its height here.
  const style = document.createElement('style')
  style.textContent = '[data-slot="scroll-area-viewport"] { height: 100% }'
  document.head.append(style)
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })
  root.render(
    <ScrollArea style={{ height: 400 }}>
      <Consumer />
    </ScrollArea>,
  )
  return { container, renders, seen }
}

async function scrollTo(viewport: HTMLElement, top: number) {
  viewport.scrollTop = top
  // Scroll events fire on the next frame; a second frame lets React commit.
  await nextFrame()
  await nextFrame()
}

test('scroll consumers re-render only when a scroll flag changes', async ({
  onTestFinished,
}) => {
  const { container, renders, seen } = mount(onTestFinished)
  await expect.poll(() => renders.count).toBeGreaterThan(0)
  const viewport = container.querySelector<HTMLElement>(
    '[data-slot="scroll-area-viewport"]',
  )!

  // Keep a continuous gesture even when CI frames exceed the 50ms idle delay.
  // Browser scroll events and React still run on real animation frames.
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  onTestFinished(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  await scrollTo(viewport, 100)
  const before = renders.count
  for (let top = 200; top <= 3000; top += 100) {
    await scrollTo(viewport, top)
  }
  expect(renders.count).toBe(before)

  const last = seen.at(-1)!
  expect(last.isScrolling).toBe(true)
  expect(last.isTop).toBe(false)
  expect(last.isBottom).toBe(false)
  expect(last.scrollDirection).toBe('down')

  await scrollTo(viewport, 4000)
  expect(seen.at(-1)!.isBottom).toBe(true)

  await scrollTo(viewport, 0)
  expect(seen.at(-1)!.isTop).toBe(true)
  expect(seen.at(-1)!.scrollDirection).toBe('up')

  flushSync(() => vi.advanceTimersByTime(49))
  expect(seen.at(-1)!.isScrolling).toBe(true)
  flushSync(() => vi.advanceTimersByTime(1))
  expect(seen.at(-1)!.isScrolling).toBe(false)
})
