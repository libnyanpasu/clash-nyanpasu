import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { FilterChip } from '@nyanpasu/ui/chip'

function mount(
  onTestFinished: (fn: () => void) => void,
  {
    disabled = false,
    onPressedChange,
  }: {
    disabled?: boolean
    onPressedChange?: (pressed: boolean) => void
  } = {},
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  function Harness() {
    const [pressed, setPressed] = useState(false)
    return (
      <FilterChip
        pressed={pressed}
        disabled={disabled}
        onPressedChange={(next) => {
          onPressedChange?.(next)
          setPressed(next)
        }}
      >
        Hide unavailable
      </FilterChip>
    )
  }
  root.render(<Harness />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  return container
}

test('toggles with a click and with Space', async ({ onTestFinished }) => {
  const onPressedChange = vi.fn()
  mount(onTestFinished, { onPressedChange })
  const chip = page.getByRole('button', { name: 'Hide unavailable' })
  await expect.element(chip).toHaveAttribute('aria-pressed', 'false')

  await userEvent.click(chip)
  expect(onPressedChange).toHaveBeenLastCalledWith(true)
  await expect.element(chip).toHaveAttribute('aria-pressed', 'true')

  await userEvent.keyboard(' ')
  expect(onPressedChange).toHaveBeenLastCalledWith(false)
  await expect.element(chip).toHaveAttribute('aria-pressed', 'false')
})

test('renders the check icon only while pressed', async ({
  onTestFinished,
}) => {
  const container = mount(onTestFinished)
  const chip = page.getByRole('button', { name: 'Hide unavailable' })
  await expect.element(chip).toBeInTheDocument()
  expect(container.querySelector('[data-slot="filter-chip"]')).not.toBeNull()
  expect(container.querySelector('[data-slot="filter-chip-check"]')).toBeNull()

  await userEvent.click(chip)
  await expect
    .poll(() => container.querySelector('[data-slot="filter-chip-check"]'))
    .not.toBeNull()

  await userEvent.click(chip)
  await expect
    .poll(() => container.querySelector('[data-slot="filter-chip-check"]'))
    .toBeNull()
})

test('disabled chip does not toggle', async ({ onTestFinished }) => {
  const onPressedChange = vi.fn()
  mount(onTestFinished, { disabled: true, onPressedChange })
  const chip = page.getByRole('button', { name: 'Hide unavailable' })
  await expect.element(chip).toBeDisabled()
  await userEvent.click(chip, { force: true })
  expect(onPressedChange).not.toHaveBeenCalled()
  await expect.element(chip).toHaveAttribute('aria-pressed', 'false')
})
