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
  for (const control of [close!, resize!, configWrapper]) {
    expect(control.getAnimations().length).toBeGreaterThan(0)
    expect(
      control
        .getAnimations()
        .some(
          (animation) => animation.effect?.getComputedTiming().duration === 200,
        ),
    ).toBe(true)
  }

  controls.setEditing(false)
  await expect.poll(() => close!.disabled).toBe(true)
  expect(close!.getAttribute('aria-hidden')).toBe('true')
  expect(resize!.getAttribute('aria-hidden')).toBe('true')
  expect(getComputedStyle(resize!).pointerEvents).toBe('none')
  expect(configWrapper.inert).toBe(true)
  expect(close!.getAnimations().length).toBeGreaterThan(0)
  expect(resize!.getAnimations().length).toBeGreaterThan(0)
  expect(configWrapper.getAnimations().length).toBeGreaterThan(0)

  await expect.poll(() => close!.isConnected).toBe(false)
  expect(resize!.isConnected).toBe(false)
  expect(config!.isConnected).toBe(false)
})
