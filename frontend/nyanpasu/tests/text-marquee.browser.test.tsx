import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import TextMarquee from '@/components/ui/text-marquee'

const LONG = 'a marquee text that is far wider than its container'

// Browser tests load no Tailwind; these are the utilities the marquee uses.
const STYLES = `
  .overflow-hidden { overflow: hidden }
  .truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
  .flex { display: flex }
  .whitespace-nowrap { white-space: nowrap }
`

function mount(onTestFinished: (fn: () => void) => void) {
  const style = document.createElement('style')
  style.textContent = STYLES
  document.head.append(style)
  const container = document.createElement('div')
  container.style.width = '100px'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })
  return { container, root }
}

const content = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('[data-slot="text-marquee-content"]')!

test('an overflowing marquee scrolls on the compositor, not per frame', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(<TextMarquee pauseDuration={0}>{LONG}</TextMarquee>)

  await expect.poll(() => content(container)?.children.length).toBe(2)

  let styleWrites = 0
  const observer = new MutationObserver(() => (styleWrites += 1))
  observer.observe(content(container), { attributeFilter: ['style'] })
  onTestFinished(() => observer.disconnect())

  await new Promise((resolve) => setTimeout(resolve, 500))

  expect(styleWrites).toBe(0)
  const animations = content(container).getAnimations()
  expect(animations).toHaveLength(1)
  expect(animations[0].playState).toBe('running')
})

test('a fitting marquee does not animate', async ({ onTestFinished }) => {
  const { container, root } = mount(onTestFinished)
  root.render(<TextMarquee>short</TextMarquee>)

  await expect.poll(() => content(container)?.textContent).toBe('short')
  await new Promise((resolve) => setTimeout(resolve, 100))
  expect(content(container).getAnimations()).toHaveLength(0)
})

test('parent re-renders do not re-measure an unchanged text', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  const observe = vi.spyOn(ResizeObserver.prototype, 'observe')
  onTestFinished(() => observe.mockRestore())

  let rerender: (text: string) => void = () => {}
  const Parent = () => {
    const [text, setText] = useState('short')
    const [, setTick] = useState(0)
    rerender = (next) => {
      setText(next)
      setTick((tick) => tick + 1)
    }
    return (
      <TextMarquee>
        <span>{text}</span>
      </TextMarquee>
    )
  }
  root.render(<Parent />)
  await expect.poll(() => content(container)?.textContent).toBe('short')
  const scrollWidth = vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get')
  onTestFinished(() => scrollWidth.mockRestore())
  observe.mockClear()

  for (let i = 0; i < 5; i++) {
    flushSync(() => rerender('short'))
  }
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(observe).not.toHaveBeenCalled()
  expect(scrollWidth).not.toHaveBeenCalled()

  // A changed text is still measured and starts the marquee.
  flushSync(() => rerender(LONG))
  await expect.poll(() => content(container).getAnimations().length).toBe(1)
})

test('a marquee keeps scrolling once its content leaves the container', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <TextMarquee pauseDuration={0} speed={1000}>
      {LONG}
    </TextMarquee>,
  )

  await expect.poll(() => content(container)?.getAnimations().length).toBe(1)
  const animation = content(container).getAnimations()[0]

  // The 100 px wide content box leaves the container's clip at -100 px,
  // which once paused the animation there for good.
  const offset = () =>
    content(container).getBoundingClientRect().left -
    container.getBoundingClientRect().left
  await expect.poll(offset, { timeout: 2000 }).toBeLessThan(-150)
  const time = Number(animation.currentTime)
  await new Promise((resolve) => setTimeout(resolve, 100))

  expect(animation.playState).toBe('running')
  expect(Number(animation.currentTime)).toBeGreaterThan(time)
})
