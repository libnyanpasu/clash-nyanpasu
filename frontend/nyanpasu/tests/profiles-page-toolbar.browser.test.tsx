import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import PageToolbar from '../src/pages/(main)/main/profiles/_modules/page-toolbar'

const longTitle = 'A very long transform pipeline name '.repeat(6).trim()

async function renderToolbar(width: number, title = 'Global transforms') {
  await page.viewport(width, 800)

  const root = createRootRoute({
    component: () => (
      <PageToolbar
        title={title}
        backTo="/main/profiles"
        context={<span data-testid="context">Applies to: My Subscription</span>}
        actions={
          <>
            <button type="button" className="w-24">
              Reset
            </button>
            <button type="button" className="w-24">
              Apply
            </button>
            <button type="button" className="w-24">
              Inspect
            </button>
          </>
        }
      />
    ),
  })
  const router = createRouter({
    routeTree: root,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  const view = await render(<RouterProvider router={router as never} />)

  const toolbar = await vi.waitUntil(() =>
    view.container.querySelector<HTMLElement>('[data-slot="page-toolbar"]'),
  )

  return { view, toolbar }
}

const cleanup = (view: { unmount: () => Promise<void> | void }) => async () => {
  await view.unmount()
  await page.viewport(1280, 800)
}

test('the back link shows on a narrow viewport only', async ({
  onTestFinished,
}) => {
  const narrow = await renderToolbar(375)
  onTestFinished(cleanup(narrow.view))

  const link = narrow.view.getByRole('link', { name: m.profile_back() })
  await expect.element(link).toBeVisible()
  expect(link.element().getAttribute('href')).toBe('/main/profiles')

  await narrow.view.unmount()

  const wide = await renderToolbar(1024)
  onTestFinished(cleanup(wide.view))

  const hidden = wide.view.container.querySelector('a[href="/main/profiles"]')!
  expect(getComputedStyle(hidden).display).toBe('none')
})

test('a long title truncates to one line inside the toolbar', async ({
  onTestFinished,
}) => {
  const { view, toolbar } = await renderToolbar(375, longTitle)
  onTestFinished(cleanup(view))

  const title = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-title"]',
  )!

  expect(title.tagName).toBe('H1')
  expect(getComputedStyle(title).textOverflow).toBe('ellipsis')
  expect(title.scrollWidth).toBeGreaterThan(title.clientWidth)
  expect(title.getBoundingClientRect().height).toBeLessThan(40)
  expect(title.getBoundingClientRect().right).toBeLessThanOrEqual(
    toolbar.getBoundingClientRect().right,
  )
  expect(toolbar.scrollWidth).toBeLessThanOrEqual(toolbar.clientWidth)
})

test('the actions wrap below the title instead of overflowing', async ({
  onTestFinished,
}) => {
  const { view, toolbar } = await renderToolbar(375)
  onTestFinished(cleanup(view))

  const title = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-title"]',
  )!
  const actions = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-actions"]',
  )!

  expect(toolbar.scrollWidth).toBeLessThanOrEqual(toolbar.clientWidth)
  expect(actions.getBoundingClientRect().right).toBeLessThanOrEqual(
    toolbar.getBoundingClientRect().right,
  )
  expect(actions.getBoundingClientRect().top).toBeGreaterThanOrEqual(
    title.getBoundingClientRect().bottom,
  )
  expect(toolbar.getBoundingClientRect().height).toBeGreaterThanOrEqual(56)
})

test('a wide toolbar keeps the title, context and actions on one row', async ({
  onTestFinished,
}) => {
  const { view, toolbar } = await renderToolbar(1280)
  onTestFinished(cleanup(view))

  const title = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-title"]',
  )!
  const context = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-context"]',
  )!
  const actions = toolbar.querySelector<HTMLElement>(
    '[data-slot="page-toolbar-actions"]',
  )!

  expect(toolbar.getBoundingClientRect().height).toBe(56)
  expect(title.getBoundingClientRect().right).toBeLessThanOrEqual(
    context.getBoundingClientRect().left,
  )
  expect(context.getBoundingClientRect().right).toBeLessThanOrEqual(
    actions.getBoundingClientRect().left,
  )
  expect(actions.getBoundingClientRect().right).toBeCloseTo(
    toolbar.getBoundingClientRect().right - 12,
    0,
  )
})
