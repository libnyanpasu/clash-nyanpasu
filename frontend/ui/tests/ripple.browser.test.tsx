import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  Ripple,
  RIPPLE_ENTER_DURATION,
  RIPPLE_EXIT_DURATION,
  rippleProgress,
  useRipple,
} from '@nyanpasu/ui/ripple'

test('a held ripple enters to half progress and stays there', () => {
  expect(rippleProgress(0, 0)).toBe(0)
  expect(rippleProgress(RIPPLE_ENTER_DURATION, 0)).toBe(0.5)
  expect(rippleProgress(10_000, 0)).toBe(0.5)
})

test('a quick release still finishes the enter before exiting', () => {
  const releasedAt = 50

  expect(rippleProgress(RIPPLE_ENTER_DURATION, 0, releasedAt)).toBe(0.5)
  expect(
    rippleProgress(
      RIPPLE_ENTER_DURATION + RIPPLE_EXIT_DURATION / 2,
      0,
      releasedAt,
    ),
  ).toBeCloseTo(0.75)
  expect(
    rippleProgress(RIPPLE_ENTER_DURATION + RIPPLE_EXIT_DURATION, 0, releasedAt),
  ).toBe(1)
})

test('a late release exits from the release time', () => {
  const releasedAt = 2000

  expect(rippleProgress(releasedAt, 0, releasedAt)).toBe(0.5)
  expect(rippleProgress(releasedAt + RIPPLE_EXIT_DURATION, 0, releasedAt)).toBe(
    1,
  )
})

function Harness() {
  const ripple = useRipple()
  const { onClick: handleClick, onPointerDown: handlePointerDown } = ripple

  return (
    <button
      type="button"
      onClick={handleClick}
      onPointerDown={handlePointerDown}
      style={{ position: 'relative', width: 200, height: 100 }}
    >
      Host
      <output data-testid="ripples">
        {JSON.stringify(
          ripple.ripples.map(({ x, y, size, releasedAt }) => ({
            x,
            y,
            size,
            held: releasedAt == null,
          })),
        )}
      </output>
    </button>
  )
}

async function mountHarness() {
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())

  const button = view.getByRole('button').element()
  const rect = button.getBoundingClientRect()

  return { view, button, rect }
}

test('a press holds a ripple at the pointer until the pointer is released', async () => {
  const { view, button, rect } = await mountHarness()

  button.dispatchEvent(
    new PointerEvent('pointerdown', {
      bubbles: true,
      button: 0,
      clientX: rect.left + 30,
      clientY: rect.top + 20,
    }),
  )

  await expect
    .element(view.getByTestId('ripples'))
    .toHaveTextContent(
      JSON.stringify([{ x: -70, y: -80, size: 200, held: true }]),
    )

  window.dispatchEvent(new PointerEvent('pointerup'))

  await expect
    .element(view.getByTestId('ripples'))
    .toHaveTextContent(
      JSON.stringify([{ x: -70, y: -80, size: 200, held: false }]),
    )
})

test('a keyboard click ripples from the center without holding', async () => {
  const { view, button } = await mountHarness()

  button.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 }))

  await expect
    .element(view.getByTestId('ripples'))
    .toHaveTextContent(
      JSON.stringify([{ x: 0, y: -50, size: 200, held: false }]),
    )
})

test('a pointer click adds no ripple beyond its press', async () => {
  const { view, button } = await mountHarness()

  button.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }))

  await expect.element(view.getByTestId('ripples')).toHaveTextContent('[]')
})

test('renders nothing without ripples and clears a finished ripple', async () => {
  let cleared: unknown = null
  const view = await render(
    <div style={{ position: 'relative', width: 100, height: 40 }}>
      <Ripple
        ripples={[
          { key: 1, x: 0, y: 0, size: 100, startedAt: 0, releasedAt: 0 },
        ]}
        onClear={(key) => (cleared = key)}
      />
    </div>,
  )
  onTestFinished(() => view.unmount())

  await expect.poll(() => cleared).toBe(1)

  const empty = await render(<Ripple ripples={[]} onClear={() => {}} />)
  onTestFinished(() => empty.unmount())

  expect(empty.container.childElementCount).toBe(0)
})
