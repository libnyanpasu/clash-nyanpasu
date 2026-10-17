import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import { Slider } from '@nyanpasu/ui/slider'

async function mount({ disabled = false, controlled = false } = {}) {
  const onValueChange = vi.fn()
  const onValueCommit = vi.fn()

  function Example() {
    const [value, setValue] = useState(50)

    return (
      <div style={{ width: 400 }}>
        <Slider
          aria-label="Volume"
          defaultValue={50}
          value={controlled ? value : undefined}
          disabled={disabled}
          onValueChange={(nextValue) => {
            setValue(nextValue)
            onValueChange(nextValue)
          }}
          onValueCommit={onValueCommit}
        />
        <button style={{ marginLeft: 440 }}>Next</button>
      </div>
    )
  }

  const view = await render(<Example />)
  onTestFinished(() => view.unmount())

  const slider = view.getByRole('slider', { name: 'Volume' })
  const input = slider.element() as HTMLInputElement
  const root = input.closest('[data-slot="slider"]')!
  const thumb = root.querySelector<HTMLElement>('[data-slot="slider-thumb"]')!

  return { view, slider, input, root, thumb, onValueChange, onValueCommit }
}

for (const controlled of [false, true]) {
  test(`track clicks align with the visible handle immediately (${controlled ? 'controlled' : 'uncontrolled'})`, async () => {
    const { slider, input, thumb, onValueCommit } = await mount({ controlled })
    const bounds = input.getBoundingClientRect()

    // Native and visible handles must share the same 48px touch target geometry.
    for (const x of [300, 100, 200, 0, 399]) {
      await slider.click({ position: { x, y: 24 } })

      const expectedValue = Math.round(
        Math.max(0, Math.min(1, (x - 24) / (bounds.width - 48))) * 100,
      )
      expect(input.valueAsNumber).toBe(expectedValue)
      const ratio = input.valueAsNumber / 100
      const expectedX = bounds.left + 24 + (bounds.width - 48) * ratio
      const handle = thumb.getBoundingClientRect()
      expect(Math.abs(handle.left + handle.width / 2 - expectedX)).toBeLessThan(
        1,
      )
      expect(onValueCommit).toHaveBeenLastCalledWith(input.valueAsNumber)
    }

    expect(input.valueAsNumber).toBe(100)
    expect(onValueCommit).toHaveBeenCalledTimes(5)
  })
}

test('dragging follows the pointer and releasing outside commits the final value once', async () => {
  const { view, slider, input, onValueChange, onValueCommit } = await mount({
    controlled: true,
  })

  await userEvent.dragAndDrop(slider, slider, {
    sourcePosition: { x: 200, y: 24 },
    targetPosition: { x: 300, y: 24 },
  })
  expect(input.valueAsNumber).toBe(78)
  expect(onValueChange).toHaveBeenLastCalledWith(78)
  expect(onValueCommit).toHaveBeenCalledExactlyOnceWith(78)

  await userEvent.dragAndDrop(
    slider,
    view.getByRole('button', { name: 'Next' }),
    {
      sourcePosition: { x: 300, y: 24 },
    },
  )
  expect(input.valueAsNumber).toBe(100)
  expect(onValueCommit.mock.calls).toEqual([[78], [100]])
})

test('keyboard changes commit once and blur does not submit again', async () => {
  const { view, input, onValueChange, onValueCommit } = await mount()
  input.focus()

  await userEvent.keyboard('{ArrowRight}')
  expect(input.valueAsNumber).toBe(51)
  expect(onValueChange).toHaveBeenLastCalledWith(51)
  expect(onValueCommit).toHaveBeenCalledExactlyOnceWith(51)

  await view.getByRole('button', { name: 'Next' }).click()
  expect(onValueCommit).toHaveBeenCalledTimes(1)

  input.focus()
  await userEvent.keyboard('{Home}{End}')
  expect(input.valueAsNumber).toBe(100)
  expect(onValueCommit.mock.calls).toEqual([[51], [0], [100]])
})

test('disabled sliders do not change or commit', async () => {
  const { slider, input, onValueChange, onValueCommit } = await mount({
    disabled: true,
  })

  await expect.element(slider).toBeDisabled()
  input.focus()
  await userEvent.keyboard('{ArrowRight}')

  expect(input.valueAsNumber).toBe(50)
  expect(onValueChange).not.toHaveBeenCalled()
  expect(onValueCommit).not.toHaveBeenCalled()
})
