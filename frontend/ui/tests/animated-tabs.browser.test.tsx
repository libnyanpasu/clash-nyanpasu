import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import AnimatedTabs, { AnimatedTabsItem } from '@nyanpasu/ui/animated-tabs'

const TABS = ['a', 'b', 'c']

const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

// `byValue` selects through the tabs' `value`; otherwise each item gets
// `isActive`, as the navbar does.
function mount(byValue: boolean, onTestFinished: (fn: () => void) => void) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })

  const controls = {
    rerender: () => {},
  }
  const Owner = () => {
    const [active] = useState('a')
    const [, setTick] = useState(0)
    controls.rerender = () => setTick((tick) => tick + 1)
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
}
