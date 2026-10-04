import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { WidgetId } from '@/components/widgets/widget-config'
import WidgetItem from '@/components/widgets/widget-item'
import { DndContext } from '@dnd-kit/core'

vi.mock('@/components/widgets/widget-config-menu', () => ({
  default: () => (
    <button
      type="button"
      className="pointer-events-auto"
      data-slot="widget-config-trigger"
    >
      Configure widget
    </button>
  ),
}))

test('edit controls animate in and become inert while exiting', async ({
  onTestFinished,
}) => {
  const items = [{ id: 'widget', x: 0, y: 0, w: 4, h: 3 }]
  const controls = { setEditing: (_editing: boolean) => {} }
  function Scene() {
    const [editing, setEditing] = useState(false)
    controls.setEditing = setEditing
    return (
      <DndContext>
        <DndGridProvider
          value={{
            displayItems: items,
            getItemRect: () => ({ left: 0, top: 0, width: 320, height: 240 }),
            dropInfoMap: {},
            activeItemId: null,
            resizingItemId: null,
            disabled: !editing,
            sourceOnly: false,
            dragIdPrefix: '',
            isOverlay: false,
            constraintsMapRef: { current: {} },
            onResizeStart: () => {},
            onResizeMove: () => {},
            onResizeEnd: () => {},
          }}
        >
          <WidgetItem id="widget" widgetType={WidgetId.ProxyShortcuts}>
            <div>Widget</div>
          </WidgetItem>
        </DndGridProvider>
      </DndContext>
    )
  }

  const screen = await render(<Scene />)
  onTestFinished(() => screen.unmount())

  const exitStates = new Map<
    Element,
    {
      connected: boolean
      hidden: boolean
      disabled: boolean
      inert: boolean
      pointerEvents: string
    }
  >()
  let exiting = false
  const nativeAnimate = Element.prototype.animate
  // Keep the real animations, including ones that finish before an assertion
  // resumes. DOM insertion can precede Motion's animation registration.
  const animate = vi
    .spyOn(Element.prototype, 'animate')
    .mockImplementation(function (
      this: Element,
      ...args: Parameters<Element['animate']>
    ) {
      const animation = nativeAnimate.apply(this, args)
      if (exiting) {
        exitStates.set(this, {
          connected: this.isConnected,
          hidden: this.getAttribute('aria-hidden') === 'true',
          disabled: this instanceof HTMLButtonElement && this.disabled,
          inert: this instanceof HTMLElement && this.inert,
          pointerEvents: getComputedStyle(this).pointerEvents,
        })
      }
      return animation
    })
  onTestFinished(() => animate.mockRestore())
  const hasControlAnimation = (element: Element) =>
    animate.mock.results.some(
      (result, index) =>
        animate.mock.contexts[index] === element &&
        result.type === 'return' &&
        result.value.effect?.getComputedTiming().duration === 200,
    )

  controls.setEditing(true)
  await expect
    .poll(() => document.querySelector('button[aria-label]'))
    .not.toBeNull()
  await expect
    .poll(() => document.querySelector('[data-slot="resize-handle"]'))
    .not.toBeNull()
  const close = document.querySelector<HTMLButtonElement>('button[aria-label]')
  const resize = document.querySelector<HTMLElement>(
    '[data-slot="resize-handle"]',
  )
  const config = document.querySelector<HTMLElement>(
    '[data-slot="widget-config-trigger"]',
  )
  expect(close).not.toBeNull()
  expect(resize).not.toBeNull()
  expect(config).not.toBeNull()
  const configWrapper = config!.parentElement!
  const elements = [close!, resize!, configWrapper]
  await expect.poll(() => elements.every(hasControlAnimation)).toBe(true)

  animate.mockClear()
  exiting = true
  controls.setEditing(false)
  await expect.poll(() => elements.every(hasControlAnimation)).toBe(true)
  expect(exitStates.get(close!)).toMatchObject({
    connected: true,
    hidden: true,
    disabled: true,
  })
  expect(exitStates.get(resize!)).toMatchObject({
    connected: true,
    hidden: true,
    pointerEvents: 'none',
  })
  expect(exitStates.get(configWrapper)).toMatchObject({
    connected: true,
    hidden: true,
    inert: true,
  })

  await expect
    .poll(() => elements.some((element) => element.isConnected))
    .toBe(false)
})
