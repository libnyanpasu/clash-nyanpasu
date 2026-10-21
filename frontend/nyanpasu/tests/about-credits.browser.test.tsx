import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { afterEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import type {
  CreditPackage,
  OpenSourceCredits,
} from '../src/pages/(main)/main/settings/about/credits/_modules/use-open-source-credits'
import { Route } from '../src/pages/(main)/main/settings/about/credits/index'

const state = vi.hoisted(() => ({
  credits: null as OpenSourceCredits | null,
  openThat: vi.fn(),
}))

vi.mock(
  '../src/pages/(main)/main/settings/about/credits/_modules/use-open-source-credits',
  () => ({
    useOpenSourceCredits: () => ({
      data: state.credits,
      error: null,
      isPending: false,
    }),
  }),
)
vi.mock('@/utils', () => ({ formatError: String }))
vi.mock('@/utils/notification', () => ({ message: vi.fn() }))
vi.mock('@/services/rpc', () => ({
  commands: { openThat: state.openThat },
}))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  isTauri: () => true,
}))

const BUNDLED: CreditPackage[] = [
  {
    source: 'bundled',
    name: 'mihomo',
    homepage: 'https://github.com/MetaCubeX/mihomo',
    license: 'GPL-3.0-only',
    textIds: ['gpl'],
    kind: 'sidecar',
  },
  {
    source: 'bundled',
    name: 'uwp-tool',
    homepage: 'https://github.com/Kuingsmile/uwp-tool',
    textIds: [],
    kind: 'resource',
    platforms: ['windows'],
    notes: 'Provides enableLoopback.exe.',
  },
]

function credits(crates = 3): OpenSourceCredits {
  return {
    schemaVersion: 1,
    texts: { gpl: 'GNU GENERAL PUBLIC LICENSE', mit: 'MIT License' },
    packages: [
      ...BUNDLED,
      ...Array.from({ length: crates }, (_, index) => ({
        source: 'cargo' as const,
        name: `crate-${String(index).padStart(4, '0')}`,
        version: '1.0.0',
        license: 'MIT OR Apache-2.0',
        textIds: ['mit'],
      })),
      {
        source: 'npm',
        name: 'react',
        version: '19.0.0',
        license: 'MIT',
        textIds: ['mit'],
      },
    ],
  }
}

async function renderCredits() {
  const rootRoute = createRootRoute({ component: Outlet })
  const creditsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/settings/about/credits',
    component: Route.options.component,
  })
  const aboutRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/main/settings/about',
    component: () => null,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([creditsRoute, aboutRoute]),
    history: createMemoryHistory({
      initialEntries: ['/main/settings/about/credits'],
    }),
  })

  return await render(
    <ScrollArea className="h-screen w-full">
      <RouterProvider router={router} />
    </ScrollArea>,
  )
}

afterEach(() => {
  state.credits = null
  state.openThat.mockReset()
})

test('packages are searchable and filterable by source', async () => {
  state.credits = credits()
  const view = await renderCredits()

  await expect.element(view.getByText('crate-0002')).toBeVisible()
  await expect
    .element(view.getByRole('button', { name: /Rust crates\s*3/ }))
    .toBeVisible()

  await view
    .getByRole('searchbox', { name: m.settings_about_credits_search() })
    .fill('gpl')
  await expect.element(view.getByText('mihomo')).toBeVisible()
  await expect.element(view.getByText('react')).not.toBeInTheDocument()

  await view
    .getByRole('searchbox', { name: m.settings_about_credits_search() })
    .fill('')
  await view.getByRole('button', { name: /npm packages/ }).click()
  await expect.element(view.getByText('react')).toBeVisible()
  await expect.element(view.getByText('mihomo')).not.toBeInTheDocument()

  await view
    .getByRole('searchbox', { name: m.settings_about_credits_search() })
    .fill('nothing-matches')
  await expect
    .element(view.getByText(m.settings_about_credits_no_match()))
    .toBeVisible()
})

test('each source is listed under a heading with its count', async () => {
  state.credits = credits()
  const view = await renderCredits()

  const sections = () =>
    [
      ...view.container.querySelectorAll<HTMLElement>(
        '[data-slot="credits-section"]',
      ),
    ].map((section) => section.textContent)
  await expect.element(view.getByText('react')).toBeVisible()
  expect(sections()).toEqual([
    `${m.settings_about_credits_source_bundled()}2`,
    `${m.settings_about_credits_source_cargo()}3`,
    `${m.settings_about_credits_source_npm()}1`,
  ])

  const slots = [
    ...view.container.querySelectorAll('[data-slot="credits-list"] > *'),
  ].map((item) => item.getAttribute('data-slot'))
  expect(slots.slice(0, 4)).toEqual([
    'credits-section',
    'credits-row',
    'credits-row',
    'credits-section',
  ])

  await view
    .getByRole('searchbox', { name: m.settings_about_credits_search() })
    .fill('crate-0001')
  await expect
    .poll(sections)
    .toEqual([`${m.settings_about_credits_source_cargo()}1`])
})

test('the license dialog shows texts, notes and the homepage', async () => {
  state.credits = credits()
  const view = await renderCredits()

  await view.getByText('mihomo').click()
  const dialog = view.getByRole('dialog')
  await expect
    .element(dialog.getByText('GNU GENERAL PUBLIC LICENSE'))
    .toBeVisible()

  await dialog
    .getByRole('button', { name: m.settings_about_credits_homepage() })
    .click()
  expect(state.openThat).toHaveBeenCalledWith(
    'https://github.com/MetaCubeX/mihomo',
  )

  await dialog.getByRole('button', { name: m.common_close() }).click()
  await expect.element(view.getByRole('dialog')).not.toBeInTheDocument()

  await view.getByText('uwp-tool').click()
  await expect
    .element(view.getByText(m.settings_about_credits_no_license_text()))
    .toBeVisible()
  await expect
    .element(view.getByText('Provides enableLoopback.exe.'))
    .toBeVisible()
  await expect
    .element(
      view.getByText(
        m.settings_about_credits_platforms({ platforms: 'Windows' }),
      ),
    )
    .toBeVisible()
})

test('the virtual list starts below the toolbar and reaches its end', async () => {
  await page.viewport(800, 600)
  state.credits = credits(500)
  const view = await renderCredits()

  await expect.element(view.getByText('crate-0000')).toBeVisible()
  // Section headings and package rows alike.
  const rows = () =>
    view.container.querySelectorAll<HTMLElement>(
      '[data-slot="credits-list"] > *',
    )
  expect(rows().length).toBeLessThan(100)

  const list = view.container.querySelector<HTMLElement>(
    '[data-slot="credits-list"]',
  )!
  const toolbar = view.container.querySelector<HTMLElement>(
    '[data-slot="credits-toolbar"]',
  )!
  expect(list.getBoundingClientRect().top).toBeGreaterThan(
    toolbar.getBoundingClientRect().bottom,
  )
  expect(
    Math.abs(
      rows()[0].getBoundingClientRect().top - list.getBoundingClientRect().top,
    ),
  ).toBeLessThan(1)

  const viewport = view.container.querySelector<HTMLElement>(
    '[data-slot=scroll-area-viewport]',
  )!
  // Midway the rendered rows must cover the whole viewport.
  const coversViewport = () => {
    const bounds = viewport.getBoundingClientRect()
    const rects = [...rows()].map((row) => row.getBoundingClientRect())

    return (
      Math.min(...rects.map((rect) => rect.top)) <= bounds.top &&
      Math.max(...rects.map((rect) => rect.bottom)) >= bounds.bottom
    )
  }
  viewport.scrollTop = 12000
  await expect.poll(coversViewport).toBe(true)

  viewport.scrollTop = viewport.scrollHeight
  await expect.element(view.getByText('react')).toBeVisible()
  expect(
    view.getByText('react').element().getBoundingClientRect().bottom,
  ).toBeLessThanOrEqual(viewport.getBoundingClientRect().bottom)
})

test('a build without credits says so', async () => {
  const view = await renderCredits()

  await expect
    .element(view.getByText(m.settings_about_credits_missing()))
    .toBeVisible()
})
