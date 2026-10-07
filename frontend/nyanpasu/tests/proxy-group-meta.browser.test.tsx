import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { GroupMeta } from '@/pages/(main)/main/proxies/group/_modules/group-meta'

const NAME = 'A group'
const ROTATE_MS = 120

function renderMeta(width: number, rotateMs = 60000) {
  return render(
    <div style={{ width }}>
      <GroupMeta
        name={NAME}
        rotateMs={rotateMs}
        status={<span>STATUS</span>}
        speed={<span>SPEED</span>}
        speedCompact={<span>FAST</span>}
      />
    </div>,
  )
}

test('shows the status and speed together when they fit', async () => {
  const screen = await renderMeta(900)

  await expect.element(screen.getByTitle(NAME)).toBeVisible()

  const meta = screen.container.querySelector(
    '[data-slot="proxies-group-meta"]',
  )
  expect(meta?.textContent).toContain('STATUS')
  expect(meta?.textContent).toContain('SPEED')
  expect(meta?.querySelector('button')).toBeNull()
})

test('collapses into a slot that rotates on its own', async () => {
  const screen = await renderMeta(40, ROTATE_MS)

  await expect
    .poll(() => screen.container.querySelector('button'))
    .not.toBeNull()

  const button = screen.getByRole('button')
  await expect
    .poll(() => button.element().getAttribute('data-showing'))
    .toBe('status')
})

test('a click switches the collapsed slot at once', async () => {
  const screen = await renderMeta(40)

  const button = screen.getByRole('button')
  await expect
    .poll(() => button.element().getAttribute('data-showing'))
    .toBe('speed')

  await button.click()
  await expect
    .poll(() => button.element().getAttribute('data-showing'))
    .toBe('status')

  await button.click()
  await expect
    .poll(() => button.element().getAttribute('data-showing'))
    .toBe('speed')
})
