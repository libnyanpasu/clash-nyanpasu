import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ReactNode } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { ImportUrlProvider } from '@/components/profile-import/import-url-provider'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { BUILTIN_STEP_LABELS } from '@/pages/(main)/main/profiles/global/_modules/builtin-steps'
import { Route as GlobalRoute } from '@/pages/(main)/main/profiles/global/route'
import { m } from '@/paraglide/messages'
import type { ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
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

const script = (
  uid: string,
  name: string,
  runtime: 'javascript' | 'lua' = 'javascript',
): ProfileItem_Serialize => ({
  uid,
  name,
  type: 'transform',
  transform: {
    type: 'script',
    runtime,
    source: { type: 'local', binding: { type: 'managed', file: `${uid}.js` } },
  },
})

const overlay = (uid: string, name: string): ProfileItem_Serialize => ({
  uid,
  name,
  type: 'transform',
  transform: {
    type: 'overlay',
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
})

const config = (transforms: string[]): ProfileItem_Serialize => ({
  uid: 'cfg',
  name: 'Remote Profile',
  type: 'config',
  config: {
    type: 'file',
    source: { type: 'local', binding: { type: 'managed', file: 'cfg.yaml' } },
    transforms,
  },
})

const BASE_TRANSFORMS = [
  overlay('dns', 'DNS override'),
  script('self', 'useself'),
  script('warp', 'warp'),
  script('lua', 'lua helper', 'lua'),
]

const numbered = (count: number) =>
  Array.from({ length: count }, (_, index) =>
    script(`extra-${index}`, `extra ${String(index).padStart(2, '0')}`),
  )

const rootRoute = createRootRoute({
  component: () => (
    <TestQueryProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false },
          },
        })
      }
    >
      <BlockTaskProvider>
        <TooltipProvider>
          <ImportUrlProvider>
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
          </ImportUrlProvider>
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

const globalRoute = GlobalRoute.update({
  id: '/main/profiles/global',
  path: '/global',
  getParentRoute: () => profilesRoute,
} as never)

// Destinations of the page's links, reduced to a marker each.
const detailRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/$type/detail/$uid',
  component: () => <div data-testid="detail-page" className="flex-1" />,
})

const inspectRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/inspect',
  component: () => <div data-testid="inspect-page" className="flex-1" />,
})

const runtimeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/main/settings/clash/runtime',
  component: () => <div data-testid="runtime-page" className="flex-1" />,
})

type Write = { method: string; params: Record<string, unknown> }

async function open({
  width = 1600,
  height = 700,
  transforms = BASE_TRANSFORMS,
  globalTransforms = ['self'],
  scoped = ['dns'],
  active = true,
}: {
  width?: number
  height?: number
  transforms?: ProfileItem_Serialize[]
  globalTransforms?: string[]
  scoped?: string[]
  active?: boolean
} = {}) {
  const writes: Write[] = []
  let globals = globalTransforms

  mockIPC((command, args) => {
    const { method, params } = args as {
      method: string
      params: Record<string, unknown>
    }

    if (command !== 'call_rpc') throw new Error(`Unexpected command ${command}`)

    if (method === 'get_profiles') {
      return {
        current: active ? 'cfg' : null,
        valid: [],
        global_transforms: globals,
        items: [config(scoped), ...transforms],
      }
    }

    if (method === 'set_global_transforms') {
      writes.push({ method, params })
      globals = params.ids as string[]
      return { status: 'committed', value: null }
    }

    if (method === 'create_editor_window') {
      writes.push({ method, params })
      return null
    }

    throw new Error(`Unexpected RPC ${method}`)
  })
  await page.viewport(width, height)

  const router = createRouter({
    routeTree: rootRoute.addChildren([
      profilesRoute.addChildren([globalRoute, detailRoute, inspectRoute]),
      runtimeRoute,
    ]),
    history: createMemoryHistory({
      initialEntries: ['/main/profiles/global'],
    }),
  })

  const view = await render(<RouterProvider router={router as never} />)

  await expect
    .poll(() =>
      view.container.querySelector('[data-slot="profile-chain-editor"]'),
    )
    .not.toBeNull()

  const query = <T extends HTMLElement>(selector: string) =>
    view.container.querySelector<T>(selector)

  const viewportOf = (slot: string) =>
    query<HTMLElement>(
      `[data-slot="${slot}"] > [data-slot="scroll-area-viewport"]`,
    )!

  const chainIds = () =>
    Array.from(
      view.container.querySelectorAll(
        '[data-slot="profile-chain-editor-item"]',
      ),
    ).map((element) => element.getAttribute('data-uid'))

  const libraryNames = () =>
    Array.from(
      view.container.querySelectorAll(
        '[data-slot="profile-global-library-row"] [data-slot="step-row-title"]',
      ),
    ).map((element) => element.textContent)

  return {
    view,
    router,
    writes,
    query,
    chainIds,
    libraryNames,
    page: () => viewportOf('profiles-content-scroll-area'),
    body: () => viewportOf('profile-global-body'),
    pipeline: () =>
      query<HTMLElement>(
        '[data-slot="profile-global-pipeline-panel"] [data-slot="profile-global-column-scroll-area"] > [data-slot="scroll-area-viewport"]',
      )!,
    library: () =>
      query<HTMLElement>(
        '[data-slot="profile-global-library"] [data-slot="profile-global-column-scroll-area"] > [data-slot="scroll-area-viewport"]',
      )!,
    apply: () =>
      view.getByRole('button', { name: m.common_apply(), exact: true }),
    reset: () =>
      view.getByRole('button', { name: m.common_reset(), exact: true }),
    add: (name: string) =>
      view.getByRole('button', {
        name: m.profile_global_library_add({ name }),
        exact: true,
      }),
  }
}

type Ui = Awaited<ReturnType<typeof open>>

const finish = (ui: Ui) => async () => {
  await ui.view.unmount()
  clearMocks()
  await page.viewport(1280, 800)
}

const scrolls = (element: HTMLElement) =>
  element.scrollHeight > element.clientHeight

test('at the wide layout the page is fixed and the pipeline and library scroll on their own', async ({
  onTestFinished,
}) => {
  const ui = await open({
    transforms: [...BASE_TRANSFORMS, ...numbered(40)],
    globalTransforms: numbered(25).map((item) => item.uid),
  })
  onTestFinished(finish(ui))

  await expect.poll(() => ui.chainIds().length).toBe(25)

  expect(ui.page().scrollHeight).toBe(ui.page().clientHeight)
  expect(document.documentElement.scrollHeight).toBeLessThanOrEqual(
    window.innerHeight,
  )
  expect(getComputedStyle(ui.body()).overflowY).toBe('clip')
  expect(scrolls(ui.body())).toBe(false)

  const pipeline = ui.pipeline()
  const library = ui.library()

  // Side by side, three to two.
  const pipelineBox = pipeline.getBoundingClientRect()
  const libraryBox = library.getBoundingClientRect()
  expect(libraryBox.left).toBeGreaterThanOrEqual(pipelineBox.right)
  expect(pipelineBox.width).toBeGreaterThan(libraryBox.width)

  expect(scrolls(pipeline)).toBe(true)
  expect(scrolls(library)).toBe(true)

  pipeline.scrollTop = 200
  expect(pipeline.scrollTop).toBe(200)
  expect(library.scrollTop).toBe(0)
  expect(ui.page().scrollTop).toBe(0)

  library.scrollTop = 150
  expect(library.scrollTop).toBe(150)
  expect(pipeline.scrollTop).toBe(200)
})

test('below the wide layout the page does not scroll and one region scrolls the stacked panels', async ({
  onTestFinished,
}) => {
  const ui = await open({
    width: 1024,
    transforms: [...BASE_TRANSFORMS, ...numbered(40)],
    globalTransforms: numbered(10).map((item) => item.uid),
  })
  onTestFinished(finish(ui))

  await expect.poll(() => ui.chainIds().length).toBe(10)

  expect(ui.page().scrollHeight).toBe(ui.page().clientHeight)
  expect(document.documentElement.scrollHeight).toBeLessThanOrEqual(
    window.innerHeight,
  )

  const body = ui.body()
  expect(scrolls(body)).toBe(true)
  expect(scrolls(ui.pipeline())).toBe(false)
  expect(scrolls(ui.library())).toBe(false)

  const pipelineBox = ui.pipeline().getBoundingClientRect()
  const libraryBox = ui.library().getBoundingClientRect()
  expect(libraryBox.top).toBeGreaterThanOrEqual(pipelineBox.bottom)

  body.scrollTop = 300
  expect(body.scrollTop).toBe(300)
})

test('a phone-width container stacks the library under the pipeline in one scroll region', async ({
  onTestFinished,
}) => {
  const ui = await open({
    width: 375,
    height: 700,
    transforms: [...BASE_TRANSFORMS, ...numbered(20)],
  })
  onTestFinished(finish(ui))

  await expect.poll(() => ui.chainIds()).toEqual(['self'])

  expect(scrolls(ui.body())).toBe(true)
  expect(scrolls(ui.pipeline())).toBe(false)
  expect(scrolls(ui.library())).toBe(false)
  expect(ui.library().getBoundingClientRect().top).toBeGreaterThanOrEqual(
    ui.pipeline().getBoundingClientRect().bottom,
  )
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(375)
})

test('Apply and Reset float in only for a changed chain', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  // The draft trails the loaded chain by a commit, so the actions may still be
  // animating out of that first frame.
  await expect
    .poll(() => ui.query('[data-slot="profile-global-actions"]'))
    .toBeNull()
  expect(ui.query('[data-slot="profile-global-unapplied"]')).toBeNull()

  await ui.add('warp').click()

  await expect.poll(() => ui.chainIds()).toEqual(['self', 'warp'])
  await expect.element(ui.apply()).toBeVisible()
  await expect.element(ui.reset()).toBeVisible()
  await expect
    .element(page.getByText(m.profile_global_unapplied(), { exact: true }))
    .toBeVisible()

  await ui.reset().click()

  await expect.poll(() => ui.chainIds()).toEqual(['self'])
  await expect
    .poll(() => ui.query('[data-slot="profile-global-actions"]'))
    .toBeNull()
  expect(ui.query('[data-slot="profile-global-unapplied"]')).toBeNull()
  expect(ui.writes).toHaveLength(0)
})

test('Apply sends the whole global chain in its new order', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await ui.add('warp').click()
  await ui.add('lua helper').click()
  await expect.poll(() => ui.chainIds()).toEqual(['self', 'warp', 'lua'])

  await ui.apply().click()

  await expect.poll(() => ui.writes.length).toBe(1)
  expect(ui.writes[0]).toEqual({
    method: 'set_global_transforms',
    params: { ids: ['self', 'warp', 'lua'] },
  })
  await expect.element(ui.apply()).toBeDisabled()
  expect(ui.chainIds()).toEqual(['self', 'warp', 'lua'])
})

test('dragging a row reorders the chain inside the scrolling pipeline', async ({
  onTestFinished,
}) => {
  const ui = await open({
    globalTransforms: ['dns', 'self', 'warp'],
    scoped: [],
  })
  onTestFinished(finish(ui))

  await expect.poll(() => ui.chainIds()).toEqual(['dns', 'self', 'warp'])

  const row = (uid: string) =>
    ui.query<HTMLElement>(
      `[data-slot="profile-chain-editor-item"][data-uid="${uid}"]`,
    )!

  await expect
    .poll(() => {
      row('warp').focus()
      return document.activeElement === row('warp')
    })
    .toBe(true)
  await userEvent.dragAndDrop(row('warp'), row('dns'))

  await expect.poll(() => ui.chainIds()).toEqual(['warp', 'dns', 'self'])
  await expect.element(ui.apply()).toBeEnabled()
  // Auto-scroll while dragging near an edge of a long list is dnd-kit's own
  // behaviour and is not exercised here.
})

test('leaving with unapplied changes asks first; Stay keeps the page, Leave navigates', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await ui.add('warp').click()
  await expect.poll(() => ui.chainIds()).toEqual(['self', 'warp'])

  const source = () =>
    ui.query<HTMLAnchorElement>(
      '[data-slot="profile-global-pipeline-link-row"]',
    )!
  await userEvent.click(source())

  await expect
    .element(page.getByText(m.profile_global_leave_title()))
    .toBeVisible()
  await page
    .getByRole('button', { name: m.profile_global_leave_stay() })
    .click()

  await expect
    .element(page.getByText(m.profile_global_leave_title()))
    .not.toBeInTheDocument()
  expect(ui.router.state.location.pathname).toBe('/main/profiles/global')
  expect(ui.chainIds()).toEqual(['self', 'warp'])

  await userEvent.click(source())
  await page
    .getByRole('button', { name: m.profile_global_leave_confirm() })
    .click()

  await expect
    .poll(() => ui.router.state.location.pathname)
    .toBe('/main/profiles/profile/detail/cfg')
})

test('a clean page navigates without asking', async ({ onTestFinished }) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await userEvent.click(
    ui.query('[data-slot="profile-global-pipeline-link-row"]')!,
  )

  await expect
    .poll(() => ui.router.state.location.pathname)
    .toBe('/main/profiles/profile/detail/cfg')
  expect(
    page.getByText(m.profile_global_leave_title()).elements(),
  ).toHaveLength(0)
})

test('the library searches, filters by type and explains each empty list', async ({
  onTestFinished,
}) => {
  const ui = await open({ globalTransforms: [] })
  onTestFinished(finish(ui))

  await expect
    .poll(() => ui.libraryNames())
    .toEqual(['DNS override', 'useself', 'warp', 'lua helper'])

  const search = page.getByRole('searchbox', {
    name: m.profile_global_library_search(),
  })

  await search.fill('LUA')
  await expect.poll(() => ui.libraryNames()).toEqual(['lua helper'])

  await search.fill('nothing like this')
  await expect.poll(() => ui.libraryNames()).toEqual([])
  await expect
    .element(page.getByText(m.profile_global_library_no_match()))
    .toBeVisible()

  await search.fill('')
  const filter = page.getByRole('group', {
    name: m.profile_global_library_filter(),
  })

  await filter.getByRole('button', { name: 'JavaScript' }).click()
  await expect.poll(() => ui.libraryNames()).toEqual(['useself', 'warp'])

  await filter.getByRole('button', { name: 'YAML' }).click()
  await expect.poll(() => ui.libraryNames()).toEqual(['DNS override'])

  await filter.getByRole('button', { name: 'Lua' }).click()
  await search.fill('warp')
  await expect
    .element(page.getByText(m.profile_global_library_no_match()))
    .toBeVisible()
})

test('an empty library offers a new transform; a fully used one says so', async ({
  onTestFinished,
}) => {
  const none = await open({ transforms: [], globalTransforms: [], scoped: [] })
  await expect
    .element(page.getByText(m.profile_global_library_empty()))
    .toBeVisible()
  await expect
    .element(
      page
        .getByRole('region', { name: m.profile_global_library_title() })
        .getByRole('button', { name: m.profile_create_transform_title() })
        .first(),
    )
    .toBeVisible()
  await finish(none)()

  const all = await open({
    transforms: BASE_TRANSFORMS,
    globalTransforms: BASE_TRANSFORMS.map((item) => item.uid),
  })
  onTestFinished(finish(all))

  await expect
    .element(page.getByText(m.profile_chain_editor_add_empty()))
    .toBeVisible()
  expect(all.libraryNames()).toEqual([])
})

test('New transform opens the create dialog', async ({ onTestFinished }) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await ui.view
    .getByRole('button', { name: m.profile_create_transform_title() })
    .click()

  await expect.element(page.getByRole('dialog')).toBeVisible()
})

test('Open in editor window is offered by library rows and chain rows', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await ui.view
    .getByRole('button', {
      name: m.profile_global_library_open_editor({ name: 'warp' }),
    })
    .click()

  await expect.poll(() => ui.writes.length).toBe(1)
  expect(ui.writes[0]).toEqual({
    method: 'create_editor_window',
    params: { windowType: 'profile', uid: 'warp' },
  })

  await ui.view
    .getByRole('button', { name: m.profile_chain_editor_open_editor() })
    .click()

  await expect.poll(() => ui.writes.length).toBe(2)
  expect(ui.writes[1]).toEqual({
    method: 'create_editor_window',
    params: { windowType: 'profile', uid: 'self' },
  })
})

test('a chain row traces its global step in Inspect', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  const trace = page.getByRole('link', {
    name: m.profile_chain_editor_trace(),
  })
  await expect.element(trace).toBeVisible()
  expect(
    decodeURIComponent(trace.element().getAttribute('href') ?? ''),
  ).toContain('step=global:self:0')
})

test('the source and the profile transforms link to their detail pages', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  await expect
    .element(page.getByText(m.profile_global_applies_to(), { exact: false }))
    .toBeVisible()

  const hrefOf = (selector: string) =>
    ui.query<HTMLAnchorElement>(selector)?.getAttribute('href')

  const links = Array.from(
    ui.view.container.querySelectorAll<HTMLAnchorElement>(
      '[data-slot="profile-global-pipeline-link-row"]',
    ),
  ).map((link) => link.getAttribute('href'))

  // The profile's own transforms are one row that opens the config.
  expect(links).toEqual(['/main/profiles/profile/detail/cfg'])
  expect(hrefOf('[data-slot="profile-global-pipeline-scoped"]')).toBe(
    '/main/profiles/profile/detail/cfg',
  )
})

test('without an active config the pipeline says so and lists no source', async ({
  onTestFinished,
}) => {
  const ui = await open({ active: false })
  onTestFinished(finish(ui))

  await expect
    .poll(() => ui.query('[data-slot="profile-global-pipeline-source-empty"]'))
    .not.toBeNull()
  expect(ui.query('[data-slot="profile-global-pipeline-link-row"]')).toBeNull()
  expect(ui.query('[data-slot="profile-global-context"]')?.textContent).toBe(
    m.profile_global_context_none(),
  )
})

test('the Nyanpasu stage lists every built-in step and links to Runtime Config', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))

  const labels = Object.values(BUILTIN_STEP_LABELS).map((label) => label())
  expect(labels).toHaveLength(5)

  const link = ui.query<HTMLAnchorElement>(
    '[data-slot="profile-global-pipeline-builtin"]',
  )!
  expect(link.getAttribute('href')).toBe('/main/settings/clash/runtime')
  expect(link.textContent).toContain(
    m.profile_global_builtin_title({ count: labels.length }),
  )
  expect(link.textContent).toContain(labels.join(', '))
})
