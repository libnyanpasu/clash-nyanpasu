import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ReactNode } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { SnackbarProvider } from '@nyanpasu/ui/snackbar'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { Route as InspectRoute } from '@/pages/(main)/main/profiles/inspect/route'
import { m } from '@/paraglide/messages'
import type {
  OperatorTag,
  ProfileItem_Serialize,
  RuntimeInspection,
  RuntimeInspectionContent,
  RuntimeInspectionNode,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { emit } from '@tauri-apps/api/event'
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

const PROFILES: ProfileItem_Serialize[] = [
  {
    uid: 'cfg',
    name: 'Remote Profile',
    type: 'config',
    config: {
      type: 'file',
      source: { type: 'local', binding: { type: 'managed', file: 'cfg.yaml' } },
      transforms: ['dns'],
    },
  },
  {
    uid: 'dns',
    name: 'DNS override',
    type: 'transform',
    transform: {
      type: 'overlay',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'dns.yaml' },
      },
    },
  },
  {
    uid: 'self',
    name: 'useself',
    type: 'transform',
    transform: {
      type: 'script',
      runtime: 'javascript',
      source: {
        type: 'local',
        binding: { type: 'managed', file: 'self.js' },
      },
    },
  },
]

const node = (
  id: number,
  tag: OperatorTag,
  changed: string[] | null,
  extra: Partial<RuntimeInspectionNode> = {},
): RuntimeInspectionNode => ({
  id,
  tag,
  next: [id + 1],
  has_logs: false,
  max_log_level: null,
  changed_fields: changed,
  ...extra,
})

const NODES: RuntimeInspectionNode[] = [
  node(
    0,
    {
      kind: 'file_config_root',
      data: { profile_id: 'cfg', role: { kind: 'selected' } },
    },
    null,
  ),
  node(
    1,
    {
      kind: 'scoped_transform',
      data: {
        host_profile_id: 'cfg',
        role: { kind: 'selected' },
        transform_profile_id: 'dns',
        transform_kind: { type: 'overlay' },
        step_index: 0,
      },
    },
    ['dns'],
  ),
  node(
    2,
    {
      kind: 'global_transform',
      data: {
        selected_profile_id: 'cfg',
        transform_profile_id: 'self',
        transform_kind: { type: 'script', runtime: 'javascript' },
        step_index: 0,
      },
    },
    ['tun'],
    { has_logs: true, max_log_level: 'warn' },
  ),
  node(
    3,
    {
      kind: 'builtin_step',
      data: { selected_profile_id: 'cfg', step: 'field_filter' },
    },
    [],
  ),
  node(
    4,
    {
      kind: 'builtin_step',
      data: { selected_profile_id: 'cfg', step: 'managed_settings' },
    },
    ['mixed-port'],
  ),
  node(
    5,
    {
      kind: 'builtin_step',
      data: { selected_profile_id: 'cfg', step: 'core_controller' },
    },
    ['external-controller'],
    { next: [] },
  ),
]

const snapshot = (
  over: Partial<RuntimeInspection> = {},
): RuntimeInspection => ({
  snapshot_id: 'snap-latest',
  applied: false,
  effective_pending: false,
  effective_revision: null,
  revision: 'rev-1',
  target_core: 'mihomo',
  root_id: 0,
  final_id: 4,
  nodes: NODES,
  ...over,
})

// 300 plain lines with a `tun` block after line 250.
const LONG_LINES = [
  ...Array.from({ length: 250 }, (_, index) => `key-${index}: value`),
  'tun:',
  '  enable: true',
  ...Array.from({ length: 48 }, (_, index) => `tail-${index}: value`),
]

const contentOf = (nodeId: number): RuntimeInspectionContent => {
  if (nodeId === 2) {
    return {
      yaml: LONG_LINES.join('\n'),
      diff: {
        parent_id: 1,
        hunks: [
          {
            old_start: 1,
            old_lines: 300,
            new_start: 1,
            new_lines: 300,
            lines: LONG_LINES.map((line, index) =>
              index === 250 || index === 251 ? `+${line}` : ` ${line}`,
            ),
          },
        ],
      },
      logs: [{ level: 'warn', message: 'tun was enabled by a script' }],
    }
  }

  return {
    yaml: ['mixed-port: 7890', `node: ${nodeId}`].join('\n'),
    diff:
      nodeId === 0
        ? null
        : {
            parent_id: nodeId - 1,
            hunks: [
              {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                lines: [' mixed-port: 7890', `+node: ${nodeId}`],
              },
            ],
          },
    logs: [],
  }
}

const rootRoute = createRootRoute({
  component: () => (
    <TestQueryProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false } },
        })
      }
    >
      <TooltipProvider>
        <SnackbarProvider>
          <div className="flex h-dvh w-dvw flex-col overflow-hidden">
            <AnimatedOutletPreset
              className="flex min-h-0 flex-1 flex-col overflow-hidden [&>div]:min-h-0 [&>div]:flex-1"
              data-slot="app-content"
            />
          </div>
        </SnackbarProvider>
      </TooltipProvider>
    </TestQueryProvider>
  ),
})

const profilesRoute = ProfilesRoute.update({
  id: '/main/profiles',
  path: '/main/profiles',
  getParentRoute: () => rootRoute,
} as never)

const inspectRoute = InspectRoute.update({
  id: '/main/profiles/inspect',
  path: '/inspect',
  getParentRoute: () => profilesRoute,
} as never)

// Destinations of the page's links, reduced to a marker each.
const globalRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/global',
  component: () => <div data-testid="global-page" className="flex-1" />,
})

const detailRoute = createRoute({
  getParentRoute: () => profilesRoute,
  path: '/$type/detail/$uid',
  component: () => <div data-testid="detail-page" className="flex-1" />,
})

type Mode = 'ready' | 'loading' | 'error' | 'empty'

async function open({
  path = '/main/profiles/inspect',
  width = 1280,
  height = 700,
  mode = 'ready',
  nodes,
}: {
  path?: string
  width?: number
  height?: number
  mode?: Mode
  nodes?: RuntimeInspectionNode[]
} = {}) {
  const state = {
    latest: snapshot(nodes && { nodes }),
    applied: snapshot({
      ...(nodes && { nodes }),
      snapshot_id: 'snap-applied',
      applied: true,
      revision: 'rev-0',
    }),
    latestFetches: 0,
    nodeRequests: [] as { snapshotId: string; nodeId: number }[],
  }

  mockIPC(
    (command, args) => {
      const { method, params } = args as {
        method: string
        params: Record<string, unknown>
      }

      if (command !== 'call_rpc')
        throw new Error(`Unexpected command ${command}`)

      if (method === 'get_profiles') {
        return {
          current: 'cfg',
          valid: [],
          global_transforms: ['self'],
          items: PROFILES,
        }
      }

      if (
        method === 'inspect_runtime' ||
        method === 'inspect_applied_runtime'
      ) {
        if (mode === 'loading') return new Promise(() => undefined)
        if (mode === 'error') throw new Error('boom')
        if (mode === 'empty') return null
        if (method === 'inspect_runtime') state.latestFetches++
        return method === 'inspect_runtime' ? state.latest : state.applied
      }

      if (method === 'inspect_runtime_node') {
        const request = params as { snapshotId: string; nodeId: number }
        state.nodeRequests.push({
          snapshotId: request.snapshotId,
          nodeId: request.nodeId,
        })
        return contentOf(request.nodeId)
      }

      throw new Error(`Unexpected RPC ${method}`)
    },
    { shouldMockEvents: true },
  )
  await page.viewport(width, height)

  const router = createRouter({
    routeTree: rootRoute.addChildren([
      profilesRoute.addChildren([inspectRoute, globalRoute, detailRoute]),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  })

  const view = await render(<RouterProvider router={router as never} />)

  const query = <T extends HTMLElement>(selector: string) =>
    view.container.querySelector<T>(selector)

  return {
    view,
    router,
    state,
    query,
    row: (key: string) =>
      query<HTMLElement>(`[data-slot="inspect-step-rail"] [data-key="${key}"]`),
    current: () =>
      query<HTMLElement>(
        '[data-slot="inspect-step-rail"] [aria-current="step"]',
      )?.dataset.key,
    detailKey: () =>
      query<HTMLElement>('[data-slot="inspect-step-detail"]')?.dataset.key,
    tab: (name: string | RegExp) => page.getByRole('tab', { name }),
  }
}

type Ui = Awaited<ReturnType<typeof open>>

const finish = (ui: Ui) => async () => {
  await ui.view.unmount()
  clearMocks()
  await page.viewport(1280, 800)
}

const ready = (ui: Ui) =>
  expect
    .poll(() => ui.query('[data-slot="inspect-step-detail"]'))
    .not.toBeNull()

test('the final step opens on its output', async ({ onTestFinished }) => {
  const ui = await open()
  onTestFinished(finish(ui))
  await ready(ui)

  expect(ui.current()).toBe('final')
  expect(ui.detailKey()).toBe('final')
  expect(ui.query('[data-slot="inspect-step-position"]')?.textContent).toBe(
    m.inspect_step_position({
      stage: m.inspect_stage_result(),
      n: 5,
      total: 6,
    }),
  )
  await expect
    .element(ui.tab(m.inspect_tab_output()))
    .toHaveAttribute('aria-selected', 'true')
  await expect
    .poll(() => ui.query('[role="region"]')?.getAttribute('aria-label'))
    .toBe(m.inspect_yaml())
  await expect.element(page.getByText('mixed-port: 7890')).toBeVisible()

  // The stages follow the build, with the steps under them.
  expect(
    [
      ...ui.view.container.querySelectorAll(
        '[data-slot="inspect-step-rail"] [data-key]',
      ),
    ].map((element) => element.getAttribute('data-key')),
  ).toEqual([
    'root:cfg',
    'scoped:cfg:dns:0',
    'global:self:0',
    'builtin:field_filter',
    'final',
    'core',
  ])
})

test('a link selects its step and opens on the changes', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)

  expect(ui.current()).toBe('global:self:0')
  expect(ui.detailKey()).toBe('global:self:0')
  await expect
    .element(ui.tab(m.inspect_tab_changes()))
    .toHaveAttribute('aria-selected', 'true')
  await expect
    .poll(() => ui.query('[role="region"]')?.getAttribute('aria-label'))
    .toBe(m.inspect_diff())
  await expect
    .element(page.getByRole('link', { name: m.inspect_edit_in_global() }))
    .toHaveAttribute('href', '/main/profiles/global')
})

test('an unknown step falls back to the final one', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=missing' })
  onTestFinished(finish(ui))
  await ready(ui)

  expect(ui.current()).toBe('final')
  expect(ui.detailKey()).toBe('final')
})

test('choosing a step replaces the history entry and keeps the link', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))
  await ready(ui)

  const before = ui.router.history.length
  await userEvent.click(ui.row('scoped:cfg:dns:0')!)

  await expect.poll(() => ui.detailKey()).toBe('scoped:cfg:dns:0')
  expect(ui.router.state.location.search).toEqual({
    step: 'scoped:cfg:dns:0',
  })
  expect(ui.router.history.length).toBe(before)
  expect(ui.current()).toBe('scoped:cfg:dns:0')
})

test('switching the snapshot keeps the selected step', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)

  await expect
    .poll(() =>
      ui.state.nodeRequests.some(
        (request) => request.snapshotId === 'snap-latest',
      ),
    )
    .toBe(true)

  await userEvent.click(
    page.getByRole('radio', { name: m.inspect_snapshot_applied() }),
  )

  await expect
    .poll(() =>
      ui.state.nodeRequests.some(
        (request) =>
          request.snapshotId === 'snap-applied' && request.nodeId === 2,
      ),
    )
    .toBe(true)
  await expect
    .element(page.getByText(new RegExp(`^${m.inspect_applied()} · `)))
    .toBeVisible()
  expect(ui.current()).toBe('global:self:0')
  expect(ui.router.state.location.search).toEqual({ step: 'global:self:0' })
})

test('a configuration change refetches the snapshot and keeps the step', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)
  await expect.poll(() => ui.state.latestFetches).toBe(1)

  ui.state.latest = snapshot({
    snapshot_id: 'snap-latest-2',
    revision: 'rev-2',
  })
  // The page listens once the first render has run.
  await vi.waitFor(async () => {
    await emit('configuration-status-changed', {})
    expect(ui.state.latestFetches).toBeGreaterThan(1)
  })

  await expect.element(page.getByText(/rev-2/)).toBeVisible()
  await expect
    .poll(() =>
      ui.state.nodeRequests.some(
        (request) => request.snapshotId === 'snap-latest-2',
      ),
    )
    .toBe(true)
  expect(ui.current()).toBe('global:self:0')
})

test('the changed-only filter keeps the source, the result and the chosen step', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))
  await ready(ui)

  await userEvent.click(ui.row('builtin:field_filter')!)
  await expect.poll(() => ui.detailKey()).toBe('builtin:field_filter')

  await userEvent.click(
    page.getByRole('switch', { name: m.inspect_only_changed() }),
  )
  // The unchanged step stays while it is chosen.
  expect(ui.row('builtin:field_filter')).not.toBeNull()

  await userEvent.click(ui.row('final')!)
  await expect.poll(() => ui.row('builtin:field_filter')).toBeNull()
  for (const key of [
    'root:cfg',
    'scoped:cfg:dns:0',
    'global:self:0',
    'final',
    'core',
  ]) {
    expect(ui.row(key), key).not.toBeNull()
  }
})

test('a changed field scrolls the diff to its line', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)

  const chip = page.getByRole('button', { name: 'tun', exact: true })
  await expect.element(chip).toBeEnabled()
  const region = (await vi.waitUntil(() =>
    ui.query<HTMLElement>('[role="region"]'),
  ))!
  await expect
    .poll(() => region.querySelectorAll('tr[data-change]').length)
    .toBeGreaterThan(0)
  expect(region.scrollTop).toBe(0)

  await userEvent.click(chip)

  const row = (await vi.waitUntil(() =>
    ui.query<HTMLElement>('tr[data-jump]'),
  ))!
  expect(row.textContent).toContain('tun:')
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
  const box = region.getBoundingClientRect()
  const rect = row.getBoundingClientRect()
  expect(rect.top).toBeGreaterThanOrEqual(box.top - 1)
  expect(rect.bottom).toBeLessThanOrEqual(box.bottom + 1)
})

test('find in the viewer counts the matches of the current tab', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)

  const region = (await vi.waitUntil(() =>
    ui.query<HTMLElement>('[role="region"]'),
  ))!
  region.focus()
  await userEvent.keyboard('{Control>}f{/Control}')

  const find = page.getByRole('search', { name: m.inspect_find_label() })
  await expect.element(find).toBeVisible()
  await userEvent.keyboard('key-1')
  // key-1, key-10 .. key-19, key-100 .. key-199.
  await expect
    .element(page.getByText(m.inspect_find_status({ current: 1, total: 111 })))
    .toBeVisible()
})

test('the field search dims the steps that did not change a matching field', async ({
  onTestFinished,
}) => {
  const ui = await open()
  onTestFinished(finish(ui))
  await ready(ui)

  await userEvent.fill(
    page.getByRole('searchbox', { name: m.inspect_find_field() }),
    'tun',
  )

  await expect
    .poll(() =>
      [
        ...ui.view.container.querySelectorAll(
          '[data-slot="inspect-step-rail"] [data-dimmed]',
        ),
      ].map((element) => element.getAttribute('data-key')),
    )
    .toEqual([
      'root:cfg',
      'scoped:cfg:dns:0',
      'builtin:field_filter',
      'final',
      'core',
    ])
  expect(ui.row('global:self:0')?.textContent).toContain(
    m.inspect_step_matches(),
  )

  // Enter goes to the step that matches.
  await userEvent.keyboard('{Enter}')
  await expect.poll(() => ui.detailKey()).toBe('global:self:0')
})

test('the logs tab lists the entries with their level', async ({
  onTestFinished,
}) => {
  const ui = await open({ path: '/main/profiles/inspect?step=global:self:0' })
  onTestFinished(finish(ui))
  await ready(ui)

  await userEvent.click(ui.tab(/^Logs|日志|ログ/))
  const list = page.getByRole('list', { name: m.inspect_logs() })
  await expect.element(list).toBeVisible()
  await expect
    .element(list.getByText('tun was enabled by a script'))
    .toBeVisible()
  await expect.element(list.getByText('warn', { exact: true })).toBeVisible()
  expect(ui.row('global:self:0')?.getAttribute('data-status')).toBe('warn')
})

test('the page does not scroll and shows the rail beside the details', async ({
  onTestFinished,
}) => {
  const ui = await open({ width: 1280, height: 700 })
  onTestFinished(finish(ui))
  await ready(ui)

  const rail = ui.query<HTMLElement>('[data-slot="inspect-step-rail"]')!
  expect(getComputedStyle(rail).display).not.toBe('none')

  const content = ui.query<HTMLElement>(
    '[data-slot="profiles-content-scroll-area"] > [data-slot="scroll-area-viewport"]',
  )!
  expect(content.scrollHeight).toBe(content.clientHeight)
  expect(document.documentElement.scrollHeight).toBeLessThanOrEqual(
    window.innerHeight,
  )
  const pageBox = ui
    .query<HTMLElement>('[data-slot="inspect-page"]')!
    .getBoundingClientRect()
  expect(pageBox.bottom).toBeLessThanOrEqual(window.innerHeight)

  // The rail sits beside the details.
  const detail = ui.query<HTMLElement>('[data-slot="inspect-step-detail"]')!
  expect(detail.getBoundingClientRect().left).toBeGreaterThanOrEqual(
    rail.getBoundingClientRect().right - 1,
  )
})

test('on a narrow screen the steps open in a drawer', async ({
  onTestFinished,
}) => {
  const ui = await open({ width: 375, height: 700 })
  onTestFinished(finish(ui))
  await ready(ui)

  const rail = ui.query<HTMLElement>('[data-slot="inspect-step-rail"]')!
  expect(getComputedStyle(rail).display).toBe('none')
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(
    window.innerWidth,
  )

  await userEvent.click(
    page.getByRole('button', { name: m.inspect_show_steps() }),
  )
  const dialog = page.getByRole('dialog')
  await expect.element(dialog).toBeVisible()

  const row = dialog.getByRole('button', { name: /DNS override/ })
  await userEvent.click(row)

  await expect.poll(() => ui.detailKey()).toBe('scoped:cfg:dns:0')
  await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
})

// The source feeds two transforms that join again before the final step.
const BRANCHED = NODES.map((entry) => {
  if (entry.id === 0) return { ...entry, next: [1, 2] }
  if (entry.id === 1 || entry.id === 2) return { ...entry, next: [3] }
  return entry
})

test('the list draws a lane for every branch of the build', async ({
  onTestFinished,
}) => {
  const ui = await open({ nodes: BRANCHED })
  onTestFinished(finish(ui))
  await ready(ui)

  const gutters = ui.view.container.querySelectorAll(
    '[data-slot="inspect-step-rail"] [data-slot="inspect-lane-gutter"]',
  )
  expect(gutters.length).toBeGreaterThan(0)
  // Two lanes are open at once, so the gutter is two lanes wide.
  expect(
    Math.max(...[...gutters].map((g) => g.getBoundingClientRect().width)),
  ).toBe(28)
})

test('the graph shows a node per step and selects like the list', async ({
  onTestFinished,
}) => {
  const ui = await open({ nodes: BRANCHED, width: 1440, height: 800 })
  onTestFinished(finish(ui))
  await ready(ui)

  await userEvent.click(
    page.getByRole('radio', { name: m.inspect_view_graph() }),
  )
  const graph = () => ui.query('[data-slot="inspect-step-graph"]')
  await expect.poll(graph).not.toBeNull()
  expect(ui.query('[data-slot="inspect-step-rail"]')).toBeNull()

  const node = (key: string) =>
    graph()!.querySelector<HTMLElement>(`[data-key="${key}"]`)
  expect(graph()!.querySelectorAll('button[data-key]')).toHaveLength(6)
  expect(node('final')?.getAttribute('aria-current')).toBe('step')

  await userEvent.click(page.getByRole('button', { name: /DNS override/ }))
  await expect.poll(() => ui.detailKey()).toBe('scoped:cfg:dns:0')
  expect(node('scoped:cfg:dns:0')?.getAttribute('aria-current')).toBe('step')

  await userEvent.click(
    page.getByRole('radio', { name: m.inspect_view_list() }),
  )
  await expect
    .poll(() => ui.query('[data-slot="inspect-step-rail"]'))
    .not.toBeNull()
  expect(ui.current()).toBe('scoped:cfg:dns:0')
})

test('the page says when it is loading', async ({ onTestFinished }) => {
  const ui = await open({ mode: 'loading' })
  onTestFinished(finish(ui))

  await expect
    .element(page.getByRole('status').filter({ hasText: m.inspect_loading() }))
    .toBeVisible()
})

test('the page says when the snapshot could not be loaded', async ({
  onTestFinished,
}) => {
  const ui = await open({ mode: 'error' })
  onTestFinished(finish(ui))

  await expect
    .element(page.getByRole('alert').filter({ hasText: m.inspect_error() }))
    .toBeVisible()
})

test('the page says when there is no snapshot yet', async ({
  onTestFinished,
}) => {
  const ui = await open({ mode: 'empty' })
  onTestFinished(finish(ui))

  await expect.element(page.getByText(m.inspect_empty())).toBeVisible()
})
