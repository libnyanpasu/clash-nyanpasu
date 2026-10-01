import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
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

  const before = renders.count
  for (let top = 100; top <= 3000; top += 100) {
    await scrollTo(viewport, top)
  }
  // Scrolling down through the middle flips isTop, isPastHeader,
  // scrollDirection and isScrolling once each; isScrolling may toggle again
  // if a frame takes longer than the scroll-end delay.
  expect(renders.count - before).toBeLessThan(10)

  const last = seen.at(-1)!
  expect(last.isTop).toBe(false)
  expect(last.isBottom).toBe(false)
  expect(last.scrollDirection).toBe('down')

  await scrollTo(viewport, 4000)
  expect(seen.at(-1)!.isBottom).toBe(true)

  await scrollTo(viewport, 0)
  expect(seen.at(-1)!.isTop).toBe(true)
  expect(seen.at(-1)!.scrollDirection).toBe('up')
})
