import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ReactNode } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { mockIPC } from '@tauri-apps/api/mocks'
import { Route as ProfilesRoute } from '../src/pages/(main)/main/profiles/route'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})

// The outlet freezes the clash streams of a leaving page; none run here.
vi.mock('@nyanpasu/query', async (importOriginal) => {
  const Through = ({ children }: { children: ReactNode }) => children

  return {
    ...(await importOriginal<typeof import('@nyanpasu/query')>()),
    ClashWSFreezeBoundary: Through,
    ClashConnectionDetailsFreezeBoundary: Through,
  }
})

// The real profiles layout around test-only child pages, inside the outlet
// the (main) layout gives its pages.
const rootRoute = createRootRoute({
  component: () => (
    <TestQueryProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false } },
        })
      }
    >
      <BlockTaskProvider>
        <TooltipProvider>
          <SnackbarProvider>
            <ContextMenuProvider>
              <div className="flex h-dvh w-dvw flex-col overflow-hidden">
                <AnimatedOutletPreset
                  className="flex min-h-0 flex-1 flex-col overflow-hidden [&>div]:min-h-0 [&>div]:flex-1"
                  data-slot="app-content"
                />
              </div>
            </ContextMenuProvider>
          </SnackbarProvider>
        </TooltipProvider>
      </BlockTaskProvider>
    </TestQueryProvider>
  ),
})

const profilesRoute = ProfilesRoute.update({
  id: '/main/profiles',
  path: '/main/profiles',
  getParentRoute: () => rootRoute,
} as never)

function FillPage() {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-hidden"
      data-testid="fill-page"
    >
      <div className="h-14 shrink-0" />
      <div className="min-h-0 flex-1 overflow-y-auto" data-testid="fill-region">
        <div className="h-[3000px]" />
      </div>
    </div>
  )
}

function ScrollPage() {
  return (
    <div className="flex flex-1 flex-col" data-testid="scroll-page">
      <div className="h-[3000px]" />
    </div>
  )
}

const fillRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/fill',
  staticData: { layout: 'fill' },
  component: FillPage,
})

const scrollRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/scroll',
  component: ScrollPage,
})

const otherRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/main/other',
  component: () => <div data-testid="other-page" className="flex-1" />,
})

async function open(path: '/main/profiles/fill' | '/main/profiles/scroll') {
  mockIPC((command, args) => {
    const wire = args as { method: string }
    const method = command === 'call_rpc' ? wire.method : command

    if (method === 'get_profiles') {
      return { current: [], valid: [], global_transforms: [], items: [] }
    }

    throw new Error(`Unexpected RPC ${method}`)
  })
  await page.viewport(1920, 900)

  const router = createRouter({
    routeTree: rootRoute.addChildren([
      profilesRoute.addChildren([fillRoute, scrollRoute]),
      otherRoute,
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  const view = await render(<RouterProvider router={router as never} />)

  const query = <T extends HTMLElement>(selector: string) =>
    view.container.querySelector<T>(selector)

  return {
    view,
    router,
    query,
    scrollArea: () =>
      query('[data-slot="profiles-content-scroll-area"]') as HTMLElement,
    viewport: () =>
      query(
        '[data-slot="profiles-content-scroll-area"] > [data-slot="scroll-area-viewport"]',
      ) as HTMLElement,
    content: () => query('[data-slot="profiles-content"]') as HTMLElement,
  }
}

const rectOf = (element: Element) => {
  const { top, left, width, height } = element.getBoundingClientRect()

  return { top, left, width, height }
}

test('a fill page is as tall as the viewport, which does not scroll', async ({
  onTestFinished,
}) => {
  const ui = await open('/main/profiles/fill')
  onTestFinished(async () => {
    await ui.view.unmount()
    await page.viewport(1280, 800)
  })

  await expect.poll(() => ui.query('[data-testid="fill-page"]')).toBeTruthy()

  const viewport = ui.viewport()
  const content = ui.content()
  const region = ui.query('[data-testid="fill-region"]')!

  expect(ui.scrollArea().dataset.layout).toBe('fill')
  expect(viewport.clientHeight).toBeGreaterThan(0)
  expect(content.getBoundingClientRect().height).toBe(viewport.clientHeight)
  expect(viewport.scrollHeight).toBe(viewport.clientHeight)
  expect(getComputedStyle(viewport).overflowY).toBe('clip')

  viewport.scrollTop = 120
  expect(viewport.scrollTop).toBe(0)

  // The fill page uses the whole width, past the 7xl limit scroll pages keep.
  expect(content.getBoundingClientRect().width).toBe(viewport.clientWidth)
  expect(viewport.clientWidth).toBeGreaterThan(1280)

  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)
  region.scrollTop = 200
  expect(region.scrollTop).toBe(200)
  expect(viewport.scrollTop).toBe(0)
})

test('a scroll page scrolls the viewport and keeps its maximum width', async ({
  onTestFinished,
}) => {
  const ui = await open('/main/profiles/scroll')
  onTestFinished(async () => {
    await ui.view.unmount()
    await page.viewport(1280, 800)
  })

  await expect.poll(() => ui.query('[data-testid="scroll-page"]')).toBeTruthy()

  const viewport = ui.viewport()

  expect(ui.scrollArea().dataset.layout).toBe('scroll')
  expect(viewport.scrollHeight).toBeGreaterThan(viewport.clientHeight)
  viewport.scrollTop = 200
  expect(viewport.scrollTop).toBe(200)
  expect(ui.content().getBoundingClientRect().width).toBeLessThanOrEqual(1280)
  expect(ui.content().getBoundingClientRect().height).toBeGreaterThan(
    viewport.clientHeight,
  )
})

// The layout flips when the route commits; the page that leaves must keep the
// box it had for the first frame. A viewport that stops being a scroll
// container drops its scroll offset, so a scrolled page that leaves for a fill
// page keeps its size but restarts at its top: the position is checked only
// for pages that were not scrolled.
for (const [from, to, leaving, layout, scrolled] of [
  ['fill', 'scroll', 'fill-page', 'scroll', false],
  ['scroll', 'fill', 'scroll-page', 'fill', false],
  ['scroll', 'fill', 'scroll-page', 'fill', true],
] as const) {
  test(`${from} to ${to}${scrolled ? ' (scrolled)' : ''}: the layout flips and the leaving page keeps its box`, async ({
    onTestFinished,
  }) => {
    const ui = await open(`/main/profiles/${from}`)
    onTestFinished(async () => {
      await ui.view.unmount()
      await page.viewport(1280, 800)
    })

    const leavingPage = await vi.waitUntil(() =>
      ui.query(`[data-testid="${leaving}"]`),
    )
    expect(ui.scrollArea().dataset.layout).toBe(from)
    ui.viewport().scrollTop = scrolled ? 400 : 0
    const before = rectOf(leavingPage)

    const atFlip: ReturnType<typeof rectOf>[] = []
    const observer = new MutationObserver(() => {
      if (
        ui.scrollArea().dataset.layout === layout &&
        leavingPage.isConnected
      ) {
        atFlip.push(rectOf(leavingPage))
      }
    })
    observer.observe(ui.scrollArea(), {
      attributes: true,
      attributeFilter: ['data-layout'],
    })
    onTestFinished(() => observer.disconnect())

    await ui.router.navigate({ to: `/main/profiles/${to}` as never })

    await expect.poll(() => ui.scrollArea().dataset.layout).toBe(layout)
    expect(atFlip).toHaveLength(1)
    // The first frame on the new layout: same box, translation only starting.
    if (!scrolled) {
      expect(atFlip[0].top).toBeCloseTo(before.top, 0)
    }
    expect(atFlip[0].width).toBeCloseTo(before.width, 0)
    expect(atFlip[0].height).toBeCloseTo(before.height, 0)
    expect(Math.abs(atFlip[0].left - before.left)).toBeLessThan(40)

    await expect.poll(() => leavingPage.isConnected).toBe(false)
  })
}

test('leaving the profiles route keeps the fill layout on the leaving page', async ({
  onTestFinished,
}) => {
  const ui = await open('/main/profiles/fill')
  onTestFinished(async () => {
    await ui.view.unmount()
    await page.viewport(1280, 800)
  })

  const area = await vi.waitUntil(() => ui.scrollArea())
  const content = ui.content()
  const before = rectOf(content)

  await ui.router.navigate({ to: '/main/other' as never })

  await expect.poll(() => ui.query('[data-testid="other-page"]')).toBeTruthy()
  expect(area.isConnected).toBe(true)
  expect(area.dataset.layout).toBe('fill')
  expect(rectOf(content).height).toBeCloseTo(before.height, 0)
  expect(getComputedStyle(ui.viewport()).overflowY).toBe('clip')

  await expect.poll(() => area.isConnected).toBe(false)
})
