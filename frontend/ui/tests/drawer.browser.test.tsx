import { useState } from 'react'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { Drawer, DrawerClose, DrawerContent, DrawerTitle } from '../src/drawer'

function Harness() {
  const [open, setOpen] = useState(true)
  const [reopens, setReopens] = useState(0)
  const [clicks, setClicks] = useState(0)

  return (
    <>
      <button onClick={() => setClicks((value) => value + 1)}>
        Page action
      </button>
      <output data-testid="clicks">{clicks}</output>
      <output data-testid="reopens">{reopens}</output>
      <Drawer open={open} onOpenChange={setOpen}>
        <DrawerContent
          aria-describedby={undefined}
          style={{
            position: 'fixed',
            bottom: 0,
            left: 0,
            width: 300,
            height: 200,
            background: 'white',
          }}
        >
          <DrawerTitle>Test drawer</DrawerTitle>
          <button
            onClick={() => {
              setOpen(false)
              requestAnimationFrame(() => {
                setOpen(true)
                setReopens((value) => value + 1)
              })
            }}
          >
            Close and reopen
          </button>
          <DrawerClose>Close drawer</DrawerClose>
        </DrawerContent>
      </Drawer>
    </>
  )
}

test('reopening during exit keeps one modal and eventually releases page clicks', async ({
  onTestFinished,
}) => {
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())

  for (let reopen = 1; reopen <= 3; reopen++) {
    await userEvent.click(
      page.getByRole('button', { name: 'Close and reopen' }),
    )
    await expect
      .element(page.getByTestId('reopens'))
      .toHaveTextContent(String(reopen))
    await expect
      .element(page.getByRole('dialog'))
      .toHaveAttribute('data-state', 'open')
    expect(
      document.querySelectorAll('[data-slot=drawer-content]'),
    ).toHaveLength(1)
    expect(
      document.querySelectorAll('[data-slot=drawer-overlay]'),
    ).toHaveLength(1)
  }

  await userEvent.click(page.getByRole('button', { name: 'Close drawer' }))
  await expect
    .poll(() => document.querySelector('[data-slot=drawer-content]'))
    .toBeNull()
  expect(document.querySelector('[data-slot=drawer-overlay]')).toBeNull()
  expect(document.body.style.pointerEvents).not.toBe('none')
  await userEvent.click(page.getByRole('button', { name: 'Page action' }))
  await expect.element(page.getByTestId('clicks')).toHaveTextContent('1')
})
