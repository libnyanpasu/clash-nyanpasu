import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import {
  NumberStepper,
  type NumberStepperVariant,
} from '@nyanpasu/ui/number-stepper'

function mount(
  onTestFinished: (fn: () => void) => void,
  {
    disabled = false,
    variant = 'outlined',
    min = 2,
    max = 20,
    initialValue = 5,
  }: {
    disabled?: boolean
    variant?: NumberStepperVariant
    min?: number
    max?: number
    initialValue?: number
  } = {},
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  function Harness() {
    const [value, setValue] = useState(initialValue)
    return (
      <>
        <output data-testid="committed-value">{value}</output>
        <NumberStepper
          label="Sample count"
          value={value}
          min={min}
          max={max}
          decrementLabel="Decrease Sample count"
          incrementLabel="Increase Sample count"
          disabled={disabled}
          variant={variant}
          onChange={setValue}
        />
      </>
    )
  }
  root.render(<Harness />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  return container
}

test.for(['outlined', 'filled', 'tonal'] as const)(
  '%s stepper supports buttons, typed values, keyboard and boundaries',
  async (variant, { onTestFinished }) => {
    const container = mount(onTestFinished, { variant })
    const input = page.getByRole('spinbutton', { name: 'Sample count' })
    await expect.element(input).toHaveValue('5')

    await userEvent.click(
      page.getByRole('button', { name: 'Increase Sample count' }),
    )
    await expect.element(input).toHaveValue('6')

    await userEvent.clear(input)
    await userEvent.type(input, '15')
    await expect.element(input).toHaveValue('15')
    await expect
      .element(page.getByTestId('committed-value'))
      .toHaveTextContent('6')
    await userEvent.keyboard('{Enter}')
    await expect
      .element(page.getByTestId('committed-value'))
      .toHaveTextContent('15')

    await userEvent.clear(input)
    await userEvent.type(input, '999')
    await expect
      .element(page.getByTestId('committed-value'))
      .toHaveTextContent('15')
    await userEvent.keyboard('{Enter}')
    await expect.element(input).toHaveValue('20')
    await userEvent.keyboard('{ArrowUp}')
    await expect.element(input).toHaveValue('20')
    expect(
      (
        container.querySelector(
          'button[aria-label="Increase Sample count"]',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)

    await userEvent.keyboard('{Home}')
    await expect.element(input).toHaveValue('2')
  },
)

test('invalid input restores the controlled value', async ({
  onTestFinished,
}) => {
  mount(onTestFinished)
  const input = page.getByRole('spinbutton', { name: 'Sample count' })
  await userEvent.clear(input)
  await userEvent.type(input, 'nope')
  await userEvent.keyboard('{Enter}')
  await expect.element(input).toHaveValue('5')
})

test('disabled state blocks input and step buttons', async ({
  onTestFinished,
}) => {
  const container = mount(onTestFinished, { disabled: true })
  await expect
    .element(page.getByRole('spinbutton', { name: 'Sample count' }))
    .toBeDisabled()
  expect(container.querySelector('input')!.disabled).toBe(true)
  expect(
    (
      container.querySelector(
        'button[aria-label="Increase Sample count"]',
      ) as HTMLButtonElement
    ).disabled,
  ).toBe(true)
  expect(
    (
      container.querySelector(
        'button[aria-label="Decrease Sample count"]',
      ) as HTMLButtonElement
    ).disabled,
  ).toBe(true)
})

test('stepper accepts negative integers when the range allows them', async ({
  onTestFinished,
}) => {
  const container = mount(onTestFinished, { min: -10, initialValue: -2 })
  const input = page.getByRole('spinbutton', { name: 'Sample count' })
  await expect.element(input).toHaveValue('-2')
  await userEvent.clear(input)
  await userEvent.type(input, '-7{Enter}')
  await expect.element(input).toHaveValue('-7')
  await expect
    .element(page.getByTestId('committed-value'))
    .toHaveTextContent('-7')
  expect(container.querySelector('input')!.getAttribute('inputmode')).toBe(
    'decimal',
  )
})
