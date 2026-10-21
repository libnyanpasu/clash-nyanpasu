import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { userEvent } from 'vitest/browser'
import { Modal, ModalContent, ModalTitle } from '@nyanpasu/ui/modal'
import {
  SharedTooltip,
  useSharedTooltipTrigger,
} from '@nyanpasu/ui/shared-tooltip'

function Row({ label, onSelect }: { label: string; onSelect?: () => void }) {
  const tooltip = useSharedTooltipTrigger(`${label} in full`)

  return (
    <button
      type="button"
      style={{ display: 'block', height: 40, width: 200 }}
      {...tooltip}
      onClick={() => {
        tooltip.onClick()
        onSelect?.()
      }}
    >
      {label}
    </button>
  )
}

function mount(onTestFinished: (fn: () => void) => void) {
  const container = document.createElement('div')
  container.style.padding = '80px'
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  return { container, root }
}

const tooltip = () => document.querySelector<HTMLElement>('[role="tooltip"]')
const tooltips = () => document.querySelectorAll('[role="tooltip"]')
const button = (container: HTMLElement, label: string) =>
  [...container.querySelectorAll('button')].find(
    (item) => item.textContent === label,
  )!

test('a hovered row shows its tooltip and describes the row', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <SharedTooltip>
      <Row label="alpha" />
      <Row label="beta" />
    </SharedTooltip>,
  )
  await expect.poll(() => container.querySelectorAll('button')).toHaveLength(2)

  await userEvent.hover(button(container, 'alpha'))

  await expect.poll(() => tooltip()?.textContent).toBe('alpha in full')
  expect(button(container, 'alpha').getAttribute('aria-describedby')).toBe(
    tooltip()!.id,
  )

  // Moving on to the next row shows that row's tooltip without the delay.
  await userEvent.hover(button(container, 'beta'))

  await expect
    .poll(() => [...tooltips()].map((item) => item.textContent))
    .toEqual(['beta in full'])
  expect(button(container, 'alpha').hasAttribute('aria-describedby')).toBe(
    false,
  )
})

test('a tooltip closes on Escape, on a click and when its row scrolls', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <div style={{ height: 120, overflow: 'auto' }} data-testid="scroller">
      <SharedTooltip>
        <Row label="alpha" />
        <div style={{ height: 400 }} />
      </SharedTooltip>
    </div>,
  )
  await expect.poll(() => container.querySelector('button')).toBeTruthy()
  const alpha = button(container, 'alpha')

  await userEvent.hover(alpha)
  await expect.poll(tooltip).toBeTruthy()
  await userEvent.keyboard('{Escape}')
  await expect.poll(tooltip).toBeNull()

  await userEvent.unhover(alpha)
  await userEvent.hover(alpha)
  await expect.poll(tooltip).toBeTruthy()
  await userEvent.click(alpha)
  await expect.poll(tooltip).toBeNull()

  await userEvent.unhover(alpha)
  await userEvent.hover(alpha)
  await expect.poll(tooltip).toBeTruthy()
  container.querySelector('[data-testid="scroller"]')!.scrollTop = 40
  await expect.poll(tooltip).toBeNull()
})

test('keyboard focus shows a tooltip at once and blur hides it', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <SharedTooltip openDelay={10_000}>
      <Row label="alpha" />
      <Row label="beta" />
    </SharedTooltip>,
  )
  await expect.poll(() => container.querySelectorAll('button')).toHaveLength(2)

  button(container, 'alpha').focus()
  await expect.poll(() => tooltip()?.textContent).toBe('alpha in full')

  await userEvent.tab()
  await expect
    .poll(() => [...tooltips()].map((item) => item.textContent))
    .toEqual(['beta in full'])

  button(container, 'beta').blur()
  await expect.poll(tooltip).toBeNull()
})

test('Escape closes a tooltip before the dialog it is in', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <Modal open>
      <ModalContent>
        <ModalTitle>Rows</ModalTitle>
        <SharedTooltip>
          <Row label="alpha" />
        </SharedTooltip>
      </ModalContent>
    </Modal>,
  )
  await expect
    .poll(() => document.querySelector('[data-slot="modal-content"] button'))
    .toBeTruthy()
  const alpha = [
    ...document.querySelectorAll<HTMLButtonElement>(
      '[data-slot="modal-content"] button',
    ),
  ].find((item) => item.textContent === 'alpha')!

  await userEvent.hover(alpha)
  await expect.poll(tooltip).toBeTruthy()
  await userEvent.keyboard('{Escape}')

  await expect.poll(tooltip).toBeNull()
  expect(document.querySelector('[data-slot="modal-content"]')).toBeTruthy()
  expect(container.isConnected).toBe(true)
})

test('a tooltip stays open while the pointer moves onto it', async ({
  onTestFinished,
}) => {
  const { container, root } = mount(onTestFinished)
  root.render(
    <SharedTooltip>
      <Row label="alpha" />
      <div style={{ height: 200 }} />
      <span>elsewhere</span>
    </SharedTooltip>,
  )
  await expect.poll(() => container.querySelector('button')).toBeTruthy()
  const alpha = button(container, 'alpha')
  const elsewhere = container.querySelector('span')!

  await userEvent.hover(alpha)
  await expect.poll(tooltip).toBeTruthy()
  await userEvent.hover(tooltip()!)
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(tooltip()).toBeTruthy()

  await userEvent.hover(elsewhere)
  await expect.poll(tooltip).toBeNull()
})
