import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs'

// Browser tests load no Tailwind; these are the utilities the tabs use.
const STYLES = `
  .relative { position: relative }
  .absolute { position: absolute }
  .inset-0 { inset: 0 }
  .inline-flex { display: inline-flex }
  [role="tab"] { width: 100px; height: 30px }
`

const TABS = ['a', 'b', 'c']

const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

// `byValue` selects through the tabs' `value`; otherwise each item gets
// `isActive`, as the navbar does.
function mount(byValue: boolean, onTestFinished: (fn: () => void) => void) {
  const style = document.createElement('style')
  style.textContent = STYLES
  document.head.append(style)
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })

  const controls = {
    rerender: () => {},
    select: (_tab: string) => {},
  }
  const Owner = () => {
    const [active, setActive] = useState('a')
    const [, setTick] = useState(0)
    controls.rerender = () => setTick((tick) => tick + 1)
    controls.select = setActive
    return (
      <AnimatedTabs activeTab={byValue ? active : undefined}>
        {TABS.map((tab) =>
          byValue ? (
            <AnimatedTabsItem key={tab} value={tab}>
              {tab}
            </AnimatedTabsItem>
          ) : (
            <AnimatedTabsItem key={tab} isActive={tab === active}>
              {tab}
            </AnimatedTabsItem>
          ),
        )}
      </AnimatedTabs>
    )
  }
  root.render(<Owner />)
  return { container, controls }
}

const indicator = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('[data-slot="animated-tabs-indicator"]')!

const translation = (element: HTMLElement) => {
  const transform = getComputedStyle(element).transform
  return transform === 'none' ? 0 : Math.abs(new DOMMatrix(transform).m41)
}

for (const byValue of [true, false]) {
  const mode = byValue ? 'by value' : 'by isActive'

  test(`re-renders do not measure the tab indicator (${mode})`, async ({
    onTestFinished,
  }) => {
    const { container, controls } = mount(byValue, onTestFinished)
    await expect.poll(() => indicator(container)).not.toBeNull()
    await nextFrame()

    const measure = vi.spyOn(Element.prototype, 'getBoundingClientRect')
    onTestFinished(() => measure.mockRestore())
    for (let i = 0; i < 5; i++) {
      flushSync(() => controls.rerender())
      await nextFrame()
    }
    expect(measure).not.toHaveBeenCalled()
  })

  test(`the indicator slides to the selected tab (${mode})`, async ({
    onTestFinished,
  }) => {
    const { container, controls } = mount(byValue, onTestFinished)
    await expect.poll(() => indicator(container)).not.toBeNull()
    await nextFrame()

    // Motion reads performance.now() on each real frame. Freeze animation time
    // so slow CI frames cannot skip the starting or intermediate position.
    let now = performance.now()
    const clock = vi.spyOn(performance, 'now').mockImplementation(() => now)
    onTestFinished(() => clock.mockRestore())
    await nextFrame()

    flushSync(() => controls.select('c'))
    await nextFrame()
    await nextFrame()
    expect(indicator(container).parentElement!.textContent).toBe('c')
    // Two tabs to the right, so it starts 200px back.
    const start = translation(indicator(container))
    expect(start).toBeCloseTo(200)

    now += 100
    await nextFrame()
    await nextFrame()
    expect(translation(indicator(container))).toBeGreaterThan(1)
    expect(translation(indicator(container))).toBeLessThan(start)

    now += 1000
    await nextFrame()
    await nextFrame()
    expect(translation(indicator(container))).toBeLessThan(1)
  })
}
