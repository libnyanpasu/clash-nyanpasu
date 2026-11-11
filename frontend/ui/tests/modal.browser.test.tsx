import { useState } from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  Modal,
  ModalAnchor,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@nyanpasu/ui/modal'

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

  // Capture layout transforms as Motion applies them. Two frames can consume
  // most of the animation on a busy runner, before the assertion resumes.
  let openingTranslation = 0
  let closingTranslation = 0
  const observer = new MutationObserver(() => {
    const content = dialog()
    if (content) {
      openingTranslation = Math.max(openingTranslation, translation(content))
    }
    closingTranslation = Math.max(closingTranslation, translation(placeholder))
  })
  observer.observe(document.body, {
    subtree: true,
    attributes: true,
    attributeFilter: ['style'],
  })
  onTestFinished(() => observer.disconnect())

  flushSync(() => controls.setOpen(true))
  await expect.poll(() => openingTranslation).toBeGreaterThan(50)

  await expect.poll(() => translation(dialog()!)).toBeLessThan(1)

  // On close the dialog fades out while the trigger's placeholder takes
  // the shared layout back from the dialog's box.
  closingTranslation = 0
  flushSync(() => controls.setOpen(false))
  await expect.poll(() => closingTranslation).toBeGreaterThan(50)
  await expect.poll(() => translation(placeholder)).toBeLessThan(1)
  await expect.poll(dialog).toBeNull()
})

function mountNode(
  node: import('react').ReactNode,
  onTestFinished: (fn: () => void) => void,
) {
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
  flushSync(() => root.render(node))
  return container
}

const closeDialog = () =>
  dialog()!.querySelector<HTMLButtonElement>('[data-slot="modal-close"]')!

test('uncontrolled triggers keep separate origins and restore the actual opener', async ({
  onTestFinished,
}) => {
  const container = mountNode(
    <Modal>
      <ModalTrigger style={{ width: 40, height: 20 }}>first</ModalTrigger>
      <ModalTrigger style={{ width: 60, height: 30 }}>second</ModalTrigger>
      <ModalContent>
        <ModalTitle>multiple triggers</ModalTitle>
        <ModalClose data-slot="modal-close" variant="flat">
          close
        </ModalClose>
      </ModalContent>
    </Modal>,
    onTestFinished,
  )
  const triggers = [
    ...container.querySelectorAll<HTMLButtonElement>(
      '[data-slot="modal-trigger"]',
    ),
  ]
  expect(triggers[0].dataset.layoutId).not.toBe(triggers[1].dataset.layoutId)

  for (const trigger of triggers) {
    trigger.focus()
    flushSync(() => trigger.click())
    await expect
      .poll(() => dialog()?.dataset.layoutId)
      .toBe(trigger.dataset.layoutId)
    await expect
      .poll(() => dialog()?.contains(document.activeElement))
      .toBe(true)
    flushSync(() => closeDialog().click())
    await expect.poll(dialog).toBeNull()
    await expect.poll(() => document.activeElement).toBe(trigger)
  }
})

test('a persistent anchor animates a programmatic open after its menu disappears', async ({
  onTestFinished,
}) => {
  function Menu() {
    const { setOpen } = useModal()
    const [menu, setMenu] = useState(false)
    return (
      <>
        <ModalAnchor style={{ width: 40, height: 20 }}>
          <button onClick={() => setMenu(true)}>menu</button>
        </ModalAnchor>
        {menu && (
          <button
            data-testid="menu-item"
            onClick={() => {
              setMenu(false)
              setOpen(true)
            }}
          >
            create
          </button>
        )}
        <ModalContent>
          <ModalTitle>create folder</ModalTitle>
          <div style={{ width: 300, height: 200 }} />
          <ModalClose data-slot="modal-close">close</ModalClose>
        </ModalContent>
      </>
    )
  }
  const container = mountNode(
    <Modal>
      <Menu />
    </Modal>,
    onTestFinished,
  )
  await nextFrame()
  let maximumTranslation = 0
  const observer = new MutationObserver(() => {
    const content = dialog()
    if (!content) return
    const transform = getComputedStyle(content).transform
    if (transform !== 'none')
      maximumTranslation = Math.max(
        maximumTranslation,
        Math.abs(new DOMMatrix(transform).m41),
      )
  })
  observer.observe(document.body, {
    subtree: true,
    attributes: true,
    attributeFilter: ['style'],
  })
  onTestFinished(() => observer.disconnect())

  container.querySelector('button')!.focus()
  flushSync(() => container.querySelector('button')!.click())
  flushSync(() =>
    container
      .querySelector<HTMLButtonElement>('[data-testid="menu-item"]')!
      .click(),
  )
  expect(container.querySelector('[data-testid="menu-item"]')).toBeNull()
  await expect.poll(() => maximumTranslation).toBeGreaterThan(50)
  flushSync(() => closeDialog().click())
  await expect.poll(dialog).toBeNull()
  expect(container.querySelector('[data-slot="modal-anchor"]')).not.toBeNull()
})

test('pending work blocks dismissal and closes through the local context after success', async ({
  onTestFinished,
}) => {
  let finish!: () => void
  const saved = new Promise<void>((resolve) => {
    finish = resolve
  })
  function Form() {
    const { setOpen } = useModal()
    const [saving, setSaving] = useState(false)
    return (
      <>
        <ModalTrigger>open</ModalTrigger>
        <ModalContent dismissible={!saving}>
          <ModalTitle>save</ModalTitle>
          <button
            onClick={async () => {
              setSaving(true)
              await saved
              setOpen(false)
            }}
          >
            save
          </button>
          <ModalClose data-slot="modal-close" disabled={saving}>
            cancel
          </ModalClose>
        </ModalContent>
      </>
    )
  }
  const container = mountNode(
    <Modal>
      <Form />
    </Modal>,
    onTestFinished,
  )
  flushSync(() => container.querySelector('button')!.click())
  await expect.poll(dialog).not.toBeNull()
  flushSync(() => dialog()!.querySelector('button')!.click())
  expect(closeDialog().disabled).toBe(true)
  flushSync(() =>
    document.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Escape',
        bubbles: true,
        cancelable: true,
      }),
    ),
  )
  expect(dialog()?.dataset.state).toBe('open')
  finish()
  await expect.poll(dialog).toBeNull()
})

test('controlled selection survives the exit and cleanup runs afterwards', async ({
  onTestFinished,
}) => {
  const exited = vi.fn()
  function Selection() {
    const [selection, setSelection] = useState<string | null>('last folder')
    return (
      <Modal
        open={selection !== null}
        onOpenChange={(open) => !open && setSelection(null)}
      >
        <ModalContent onExitComplete={exited}>
          <ModalTitle>{selection}</ModalTitle>
          <ModalClose data-slot="modal-close">close</ModalClose>
        </ModalContent>
      </Modal>
    )
  }
  mountNode(<Selection />, onTestFinished)
  await expect.poll(dialog).not.toBeNull()
  await expect.poll(() => getComputedStyle(dialog()!).opacity).toBe('1')
  flushSync(() => closeDialog().click())
  expect(dialog()?.textContent).toContain('last folder')
  expect(exited).not.toHaveBeenCalled()
  await expect.poll(dialog).toBeNull()
  expect(exited).toHaveBeenCalledTimes(1)
})

test('reopening during exit restores focus to the new trigger', async ({
  onTestFinished,
}) => {
  const container = mountNode(
    <Modal>
      <ModalTrigger>first</ModalTrigger>
      <ModalTrigger>second</ModalTrigger>
      <ModalContent>
        <ModalTitle>reopen</ModalTitle>
        <ModalClose data-slot="modal-close">close</ModalClose>
      </ModalContent>
    </Modal>,
    onTestFinished,
  )
  const [first, second] = container.querySelectorAll<HTMLButtonElement>(
    '[data-slot="modal-trigger"]',
  )
  first.focus()
  flushSync(() => first.click())
  await expect.poll(() => getComputedStyle(dialog()!).opacity).toBe('1')

  flushSync(() => closeDialog().click())
  expect(dialog()).not.toBeNull()
  second.focus()
  flushSync(() => second.click())
  await expect
    .poll(() => dialog()?.dataset.layoutId)
    .toBe(second.dataset.layoutId)
  await expect.poll(() => getComputedStyle(dialog()!).opacity).toBe('1')
  flushSync(() => closeDialog().click())
  await expect.poll(dialog).toBeNull()
  await expect.poll(() => document.activeElement).toBe(second)
})
