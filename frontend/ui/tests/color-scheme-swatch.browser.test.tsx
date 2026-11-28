import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import {
  ColorSchemeSwatch,
  ColorSchemeSwatchGroup,
} from '@nyanpasu/ui/color-scheme-swatch'

const colors = {
  primary: '#1867c0',
  secondary: '#565f71',
  tertiary: '#715573',
  surface: '#faf9fd',
}

async function mount(onTestFinished: (fn: () => void) => void) {
  const onValueChange = vi.fn()

  function Group() {
    const [value, setValue] = useState('tonalSpot')

    return (
      <ColorSchemeSwatchGroup
        aria-label="Scheme"
        value={value}
        onValueChange={(next) => {
          setValue(next)
          onValueChange(next)
        }}
      >
        <ColorSchemeSwatch
          value="tonalSpot"
          label="Tonal Spot"
          colors={colors}
        />
        <ColorSchemeSwatch value="vibrant" label="Vibrant" colors={colors} />
        <ColorSchemeSwatch value="neutral" label="Neutral" colors={colors} />
      </ColorSchemeSwatchGroup>
    )
  }

  const view = await render(<Group />)

  onTestFinished(() => view.unmount())

  const radio = (name: string) => view.getByRole('radio', { name })

  return { view, onValueChange, radio }
}

test('the controlled value is the checked swatch', async ({
  onTestFinished,
}) => {
  const { radio } = await mount(onTestFinished)

  await expect
    .poll(() => radio('Tonal Spot').element().getAttribute('aria-checked'))
    .toBe('true')
  expect(radio('Vibrant').element().getAttribute('aria-checked')).toBe('false')
})

test('clicking a swatch selects it and emits its value', async ({
  onTestFinished,
}) => {
  const { onValueChange, radio } = await mount(onTestFinished)

  await radio('Vibrant').click()

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['vibrant'])
  await expect
    .poll(() => radio('Vibrant').element().getAttribute('aria-checked'))
    .toBe('true')
  expect(radio('Tonal Spot').element().getAttribute('aria-checked')).toBe(
    'false',
  )
})

test('arrow keys move the selection, Home and End move focus', async ({
  onTestFinished,
}) => {
  const { onValueChange, radio } = await mount(onTestFinished)

  // Radix selects on focus only while the arrow key is still held, and focus
  // lands a task after keydown, so the key is released once the value arrives.
  const arrow = async (key: string, expected: string[]) => {
    await userEvent.keyboard(`{${key}>}`)
    await expect.poll(() => onValueChange.mock.lastCall).toEqual(expected)
    await userEvent.keyboard(`{/${key}}`)
  }

  await radio('Tonal Spot').click()
  await arrow('ArrowRight', ['vibrant'])

  await arrow('ArrowDown', ['neutral'])

  await arrow('ArrowLeft', ['vibrant'])

  await userEvent.keyboard('{End} ')

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['neutral'])

  await userEvent.keyboard('{Home} ')

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['tonalSpot'])
})
