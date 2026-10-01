import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal'

// Browser tests load no Tailwind; these are the utilities the modal uses.
const STYLES = `
  .fixed { position: fixed }
  .absolute { position: absolute }
  .relative { position: relative }
  .inset-0 { inset: 0 }
  .grid { display: grid }
  .place-items-center { place-items: center }
  .size-full { width: 100%; height: 100% }
`

const nextFrame = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

function mount(onTestFinished: (fn: () => void) => void) {
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
    setOpen: (_open: boolean) => {},
  }
  const Owner = () => {
    const [open, setOpen] = useState(false)
    const [tick, setTick] = useState(0)
    controls.rerender = () => setTick((value) => value + 1)
    controls.setOpen = setOpen
    return (
      <Modal open={open} onOpenChange={setOpen}>
        <ModalTrigger style={{ width: 40, height: 20 }}>open</ModalTrigger>
        <ModalContent>
          <ModalTitle>title</ModalTitle>
          <div style={{ width: 300, height: 200 }}>{tick}</div>
        </ModalContent>
      </Modal>
    )
  }
  root.render(<Owner />)
  return controls
}

const dialog = () =>
  document.querySelector<HTMLElement>('[data-slot="modal-content"]')

test('owner re-renders do not measure the modal layout', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect
    .poll(() => document.querySelector('[data-slot="modal-trigger"]'))
    .not.toBeNull()

  const measure = vi.spyOn(Element.prototype, 'getBoundingClientRect')
  onTestFinished(() => measure.mockRestore())

  for (let i = 0; i < 5; i++) {
    flushSync(() => controls.rerender())
    await nextFrame()
  }
  expect(measure).not.toHaveBeenCalled()

  flushSync(() => controls.setOpen(true))
  // Let the open animation finish before re-rendering the open dialog.
  await new Promise((resolve) => setTimeout(resolve, 1000))
  measure.mockClear()

  for (let i = 0; i < 5; i++) {
    flushSync(() => controls.rerender())
    await nextFrame()
  }
  expect(dialog()?.textContent).toContain('10')
  expect(measure).not.toHaveBeenCalled()
})

test('the dialog grows out of its trigger and shrinks back', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect
    .poll(() => document.querySelector('[data-slot="modal-trigger"]'))
    .not.toBeNull()
  await nextFrame()

  // The trigger sits at the top-left corner and the dialog in the middle of
  // the viewport, so the shared-element animation starts with a large
  // translation toward the trigger.
  const translation = (element: HTMLElement) => {
    const transform = getComputedStyle(element).transform
    return transform === 'none' ? 0 : Math.abs(new DOMMatrix(transform).m41)
  }
  const placeholder = document.querySelector<HTMLElement>(
    '[data-slot="modal-trigger-placeholder"]',
  )!

  flushSync(() => controls.setOpen(true))
  await nextFrame()
  await nextFrame()
  expect(translation(dialog()!)).toBeGreaterThan(50)

  await new Promise((resolve) => setTimeout(resolve, 1000))
  expect(translation(dialog()!)).toBeLessThan(1)

  // On close the dialog fades out while the trigger's placeholder takes
  // the shared layout back from the dialog's box.
  flushSync(() => controls.setOpen(false))
  await nextFrame()
  await nextFrame()
  expect(translation(placeholder)).toBeGreaterThan(50)
})
