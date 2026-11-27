import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { Route as WindowPage } from '@/pages/(inspector)/candidate-config/index'
import { Route as WindowLayout } from '@/pages/(inspector)/candidate-config/route'
import { m } from '@/paraglide/messages'
import { getLocale, overwriteGetLocale } from '@/paraglide/runtime'
import type { IpcError, RuntimeInspection } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider } from './query-provider'

vi.hoisted(() => vi.stubGlobal('isTauri', true))
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())
vi.mock('@/components/window/window-control', () => ({
  default: () => <div className="h-8 w-35 shrink-0" aria-hidden />,
}))
vi.mock('@/components/logo/animated-logo', () => ({ default: () => null }))

const id = 'a'.repeat(21)
const summary: RuntimeInspection = {
  snapshot_id: id,
  revision: 'candidate-1',
  target_core: 'mihomo',
  applied: false,
  effective_pending: false,
  effective_revision: null,
  root_id: 0,
  final_id: 1,
  nodes: [
    {
      id: 0,
      tag: { kind: 'bare_root' },
      next: [1],
      has_logs: false,
      max_log_level: null,
      changed_fields: null,
    },
    {
      id: 1,
      tag: {
        kind: 'builtin_step',
        data: { selected_profile_id: null, step: 'managed_settings' },
      },
      next: [],
      has_logs: true,
      max_log_level: 'warn',
      changed_fields: ['mode'],
    },
  ],
}
const stale: IpcError = {
  kind: { domain: 'runtime', error: { kind: 'runtime_snapshot_changed' } },
  message: 'opaque',
  detail: 'opaque',
}
// The desktop transport rejects with the RPC envelope that carries the domain error.
const staleEnvelope = {
  kind: 'application_error',
  message: 'opaque',
  domain_error: stale,
}

const member = { uid: 'member', name: 'Member profile' }

async function open(
  mode: 'ready' | 'stale' | 'node-stale' | 'halted',
  onTestFinished: (cleanup: () => Promise<void>) => void,
  width = 1100,
) {
  const calls: { method: string; params: Record<string, unknown> }[] = []
  mockIPC(
    (command, args) => {
      expect(command).toBe('call_rpc')
      const call = args as (typeof calls)[number]
      // The profile names the steps show; not a runtime query.
      if (call.method === 'get_profiles_listing')
        return { items: [member], current: [], global_transforms: [] }
      calls.push(call)
      if (call.method === 'inspect_candidate_runtime') {
        expect(call.params).toEqual({ snapshotId: id })
        if (mode === 'stale') return Promise.reject(staleEnvelope)
        return {
          inspection: summary,
          halted:
            mode === 'halted'
              ? {
                  step: { kind: 'profile', profile_id: member.uid },
                  reason: 'the member file could not be read',
                }
              : null,
          stage: 'built',
        }
      }
      if (call.method === 'inspect_candidate_node') {
        expect(call.params).toEqual({ snapshotId: id, nodeId: 1 })
        if (mode === 'node-stale') return Promise.reject(staleEnvelope)
        return {
          yaml: 'mode: rule',
          diff: {
            parent_id: 0,
            hunks: [
              {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: ['-mode: direct', '+mode: rule'],
              },
            ],
          },
          logs: [{ level: 'warn', message: 'opaque build log' }],
        }
      }
      throw new Error(`Unexpected RPC ${call.method}`)
    },
    { shouldMockEvents: true },
  )
  await page.viewport(width, 720)
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const root = createRootRoute({
    component: () => (
      <TestQueryProvider client={client}>
        <TooltipProvider>
          <Outlet />
        </TooltipProvider>
      </TestQueryProvider>
    ),
  })
  const layout = WindowLayout.update({
    id: '/candidate-config',
    path: '/candidate-config',
    getParentRoute: () => root,
  } as never)
  const index = WindowPage.update({
    id: '/',
    path: '/',
    getParentRoute: () => layout,
  } as never)
  const router = createRouter({
    routeTree: root.addChildren([layout.addChildren([index])]),
    history: createMemoryHistory({
      initialEntries: [`/candidate-config?id=${id}`],
    }),
  })
  const view = await render(<RouterProvider router={router as never} />)
  onTestFinished(async () => {
    await view.unmount()
    client.clear()
    clearMocks()
    await page.viewport(1280, 800)
  })
  return { view, calls }
}

test('the candidate window loads its summary, output, diff and logs without runtime queries', async ({
  onTestFinished,
}) => {
  const ui = await open('ready', onTestFinished)
  await expect
    .element(page.getByText('mode: rule', { exact: true }))
    .toBeVisible()
  expect(ui.calls.map((call) => call.method)).toEqual([
    'inspect_candidate_runtime',
    'inspect_candidate_node',
  ])
  await expect
    .element(page.getByRole('heading', { name: m.inspect_candidate_title() }))
    .toBeVisible()
  expect(
    ui.view.container.querySelector('a[href^="/main/profiles"]'),
  ).toBeNull()
  await expect
    .element(page.getByRole('button', { name: m.inspect_refresh() }))
    .not.toBeInTheDocument()
  await expect
    .element(page.getByRole('radio', { name: m.inspect_snapshot_latest() }))
    .not.toBeInTheDocument()
  await userEvent.click(
    page.getByRole('tab', { name: m.inspect_tab_changes() }),
  )
  await expect
    .element(page.getByText('mode: direct', { exact: true }))
    .toBeVisible()
  await userEvent.click(
    page.getByRole('tab', { name: new RegExp(`^${m.inspect_tab_logs()}`) }),
  )
  await expect.element(page.getByText('opaque build log')).toBeVisible()
})

for (const mode of ['stale', 'node-stale'] as const) {
  test(`the candidate window shows unavailable when ${mode}`, async ({
    onTestFinished,
  }) => {
    const ui = await open(mode, onTestFinished)
    await expect
      .element(page.getByText(m.inspect_candidate_unavailable()))
      .toBeVisible()
    expect(
      ui.view.container.querySelector('[data-slot="inspect-step-detail"]'),
    ).toBeNull()
    expect(ui.calls.map((call) => call.method)).toEqual(
      mode === 'stale'
        ? ['inspect_candidate_runtime']
        : ['inspect_candidate_runtime', 'inspect_candidate_node'],
    )
  })
}

test.for(['en', 'ja', 'ko', 'ru', 'zh-cn', 'zh-tw'] as const)(
  'the unavailable window fits a narrow screen in %s',
  async (locale, { onTestFinished }) => {
    const previousGetLocale = getLocale
    overwriteGetLocale(() => locale)
    onTestFinished(() => overwriteGetLocale(previousGetLocale))
    const ui = await open('stale', onTestFinished, 400)
    await expect
      .element(page.getByText(m.inspect_candidate_unavailable()))
      .toBeVisible()
    await expect
      .element(page.getByRole('heading', { name: m.inspect_candidate_title() }))
      .toBeVisible()
    expect(
      ui.view.container.querySelector('[data-slot="candidate-config-window"]')!
        .scrollWidth,
    ).toBeLessThanOrEqual(400)
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(400)
  },
)

test('a stopped candidate names the step it could not run by its profile name', async ({
  onTestFinished,
}) => {
  await open('halted', onTestFinished)
  await expect
    .element(
      page.getByText(m.inspect_candidate_halted_at({ step: member.name })),
    )
    .toBeVisible()
})
