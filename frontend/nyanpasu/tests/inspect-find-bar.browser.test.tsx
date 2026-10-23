import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import ChangedFields from '../src/pages/(main)/main/profiles/inspect/_modules/changed-fields'
import {
  FindBar,
  isFindShortcut,
  revealWithin,
} from '../src/pages/(main)/main/profiles/inspect/_modules/find-bar'

function Harness({ total, onClose }: { total: number; onClose?: () => void }) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  return (
    <FindBar
      query={query}
      onQueryChange={(value) => {
        setQuery(value)
        setActive(0)
      }}
      total={total}
      active={active}
      onActiveChange={setActive}
      onClose={onClose ?? (() => {})}
      status={`${active + 1} of ${total}`}
      label="Find in text"
      clearLabel="Clear"
      previousLabel="Previous match"
      nextLabel="Next match"
      closeLabel="Close find"
    />
  )
}

test('Enter and Shift+Enter walk through the matches and wrap around', async () => {
  await render(<Harness total={3} />)
  const input = page.getByRole('searchbox', { name: 'Find in text' })
  await input.fill('a')
  const status = page.getByRole('status')
  await expect.element(status).toHaveTextContent('1 of 3')

  await userEvent.keyboard('{Enter}')
  await expect.element(status).toHaveTextContent('2 of 3')
  await userEvent.keyboard('{Enter}{Enter}')
  await expect.element(status).toHaveTextContent('1 of 3')
  await userEvent.keyboard('{Shift>}{Enter}{/Shift}')
  await expect.element(status).toHaveTextContent('3 of 3')
})

test('the buttons step through the matches', async () => {
  await render(<Harness total={2} />)
  await page.getByRole('searchbox').fill('a')
  const status = page.getByRole('status')
  await page.getByRole('button', { name: 'Next match' }).click()
  await expect.element(status).toHaveTextContent('2 of 2')
  await page.getByRole('button', { name: 'Previous match' }).click()
  await expect.element(status).toHaveTextContent('1 of 2')
})

test('Escape and the close button close the bar', async () => {
  const onClose = vi.fn()
  await render(<Harness total={1} onClose={onClose} />)
  await page.getByRole('searchbox').fill('a')
  await userEvent.keyboard('{Escape}')
  expect(onClose).toHaveBeenCalledTimes(1)
  await page.getByRole('button', { name: 'Close find' }).click()
  expect(onClose).toHaveBeenCalledTimes(2)
})

test('the steps are disabled without matches and no status shows without a query', async () => {
  await render(<Harness total={0} />)
  await expect
    .element(page.getByRole('button', { name: 'Next match' }))
    .toBeDisabled()
  await expect
    .element(page.getByRole('button', { name: 'Previous match' }))
    .toBeDisabled()
  await expect.element(page.getByRole('status')).toHaveTextContent('')
})

test('Ctrl+F and Cmd+F are the find shortcut, Shift or Alt are not', () => {
  const event = (init: object) =>
    ({
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
      altKey: false,
      key: 'f',
      ...init,
    }) as never
  expect(isFindShortcut(event({ ctrlKey: true }))).toBe(true)
  expect(isFindShortcut(event({ metaKey: true, key: 'F' }))).toBe(true)
  expect(isFindShortcut(event({}))).toBe(false)
  expect(isFindShortcut(event({ ctrlKey: true, shiftKey: true }))).toBe(false)
  expect(isFindShortcut(event({ ctrlKey: true, key: 'g' }))).toBe(false)
})

test('revealWithin scrolls only the container, and only when needed', async ({
  onTestFinished,
}) => {
  const outer = document.createElement('div')
  outer.style.cssText = 'height:300px;overflow:auto'
  const container = document.createElement('div')
  container.style.cssText = 'height:100px;overflow:auto'
  container.innerHTML = Array.from(
    { length: 20 },
    (_, i) => `<div style="height:50px" id="row-${i}">row ${i}</div>`,
  ).join('')
  outer.append(container)
  document.body.append(outer)
  onTestFinished(() => outer.remove())

  revealWithin(container, container.querySelector<HTMLElement>('#row-0')!)
  expect(container.scrollTop).toBe(0)

  revealWithin(container, container.querySelector<HTMLElement>('#row-10')!)
  // Centered: row 10 starts at 500, the container shows 100.
  expect(container.scrollTop).toBe(475)
  expect(outer.scrollTop).toBe(0)
})

test('changed fields are plain without onSelect, buttons with it', async () => {
  const view = await render(<ChangedFields fields={['dns', 'rules']} />)
  expect(view.container.querySelectorAll('button')).toHaveLength(0)
  expect(view.container.querySelectorAll('code')).toHaveLength(2)
  await view.unmount()

  const onSelect = vi.fn()
  await render(
    <ChangedFields
      fields={['dns', 'rules']}
      query="ru"
      onSelect={onSelect}
      canSelect={(field) => field !== 'dns'}
    />,
  )
  await expect.element(page.getByRole('button', { name: 'dns' })).toBeDisabled()
  const rules = page.getByRole('button', { name: 'rules' })
  await rules.click()
  expect(onSelect).toHaveBeenCalledWith('rules')
  // The part that matches the field search is marked.
  expect(document.querySelector('mark')?.textContent).toBe('ru')
})

test('expanded changed fields scroll instead of growing without bound', async () => {
  const fields = Array.from({ length: 300 }, (_, i) => `proxy-groups.${i}.name`)
  await render(<ChangedFields fields={fields} />)
  await page.getByRole('button', { name: '+292' }).click()

  const box = document.querySelector<HTMLElement>(
    '[data-slot="inspect-changed-fields"]',
  )!
  expect(box.scrollHeight).toBeGreaterThan(box.clientHeight)
  expect(box.clientHeight).toBeLessThanOrEqual(128)
})
