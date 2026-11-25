import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { DEFAULT_INSPECT_VIEW } from '@/pages/(main)/main/profiles/inspect/_modules/inspect-view'
import InspectViewer, {
  type InspectionSource,
} from '@/pages/(main)/main/profiles/inspect/_modules/inspect-viewer'
import { m } from '@/paraglide/messages'
import type {
  RuntimeInspection_Serialize,
  RuntimeInspectionContent_Serialize,
} from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'

vi.hoisted(() => {
  vi.stubGlobal('isTauri', true)
})

const summary: RuntimeInspection_Serialize = {
  snapshot_id: 'injected-build',
  applied: false,
  effective_pending: false,
  effective_revision: null,
  revision: 'injected-revision',
  target_core: 'mihomo',
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
      has_logs: false,
      max_log_level: null,
      changed_fields: ['mixed-port'],
    },
  ],
}

test('the viewer loads injected node content without runtime queries', async ({
  onTestFinished,
}) => {
  const ipc = vi.fn(() => {
    throw new Error('Unexpected backend call')
  })
  mockIPC(ipc)
  await page.viewport(1280, 700)
  const queryClient = new QueryClient()
  const loadNode = vi.fn((snapshotId: string, nodeId: number) => ({
    queryKey: ['injected-node', snapshotId, nodeId],
    queryFn: async (): Promise<RuntimeInspectionContent_Serialize> => ({
      yaml: `injected-node: ${nodeId}`,
      diff: null,
      logs: [],
    }),
  }))
  const source: InspectionSource = {
    summary: { data: summary, isPending: false, isError: false, error: null },
    loadNode,
  }

  function Viewer() {
    const [step, setStep] = useState<string>()
    const [view, setView] = useState(DEFAULT_INSPECT_VIEW)

    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <div className="flex h-dvh w-dvw flex-col">
            <InspectViewer
              source={source}
              selectedStep={step}
              onStepChange={setStep}
              inspectView={view}
              onViewChange={(update) =>
                setView((previous) => ({ ...previous, ...update }))
              }
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    )
  }

  const root = createRootRoute({ component: Viewer })
  const router = createRouter({
    routeTree: root,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  const screen = await render(<RouterProvider router={router as never} />)
  onTestFinished(async () => {
    await screen.unmount()
    queryClient.clear()
    clearMocks()
    vi.unstubAllGlobals()
    await page.viewport(1280, 800)
  })

  await expect.element(page.getByText('injected-node: 1')).toBeVisible()
  expect(loadNode).toHaveBeenCalledWith('injected-build', 1)
  expect(
    screen.container.querySelector('[data-slot="inspect-status"]'),
  ).toBeNull()
  await expect
    .element(page.getByRole('button', { name: m.inspect_refresh() }))
    .not.toBeInTheDocument()

  const sourceStep = screen.container.querySelector<HTMLElement>(
    '[data-slot="inspect-step-rail"] [data-key="root:bare"]',
  )!
  await userEvent.click(sourceStep)
  await expect
    .poll(() => loadNode.mock.calls.some(([, id]) => id === 0))
    .toBe(true)
  await userEvent.click(page.getByRole('tab', { name: m.inspect_tab_output() }))
  await expect.element(page.getByText('injected-node: 0')).toBeVisible()
  expect(ipc).not.toHaveBeenCalled()
})

test('a kept candidate says how far it got and why its build stopped', async ({
  onTestFinished,
}) => {
  mockIPC(() => {
    throw new Error('Unexpected backend call')
  })
  const queryClient = new QueryClient()
  const source: InspectionSource = {
    summary: { data: summary, isPending: false, isError: false, error: null },
    loadNode: (snapshotId, nodeId) => ({
      queryKey: ['candidate-node', snapshotId, nodeId],
      queryFn: async (): Promise<RuntimeInspectionContent_Serialize> => ({
        yaml: `candidate-node: ${nodeId}`,
        diff: null,
        logs: [],
      }),
    }),
  }
  const reason = 'the member file could not be read'
  const halted = {
    step: { kind: 'profile', profile_id: 'member' },
    reason,
  } as const
  const profiles = new Map([
    ['member', { uid: 'member', name: 'Member profile' } as never],
  ])

  function Viewer({ stopped }: { stopped: boolean }) {
    const [step, setStep] = useState<string>()

    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <div className="flex h-dvh w-dvw flex-col">
            <InspectViewer
              source={source}
              selectedStep={step}
              onStepChange={setStep}
              inspectView={DEFAULT_INSPECT_VIEW}
              onViewChange={() => {}}
              profiles={profiles}
              candidate={
                stopped
                  ? { halted, stage: 'built' }
                  : { halted: null, stage: 'applied_unpublished' }
              }
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>
    )
  }

  const render_ = async (stopped: boolean) => {
    const root = createRootRoute({
      component: () => <Viewer stopped={stopped} />,
    })
    const router = createRouter({
      routeTree: root,
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })
    return render(<RouterProvider router={router as never} />)
  }

  const stopped = await render_(true)
  onTestFinished(() => {
    queryClient.clear()
    clearMocks()
    vi.unstubAllGlobals()
  })
  // It names the step it could not run, as its rows name that profile.
  await expect
    .element(
      page.getByText(m.inspect_candidate_halted_at({ step: 'Member profile' })),
    )
    .toBeVisible()
  expect(
    stopped.container.querySelector('[data-slot="inspect-candidate-halted"]')
      ?.textContent,
  ).toContain(reason)
  // The last completed step is shown as itself, never as a final config.
  await userEvent.click(page.getByRole('tab', { name: m.inspect_tab_output() }))
  await expect.element(page.getByText('candidate-node: 1')).toBeVisible()
  expect(
    stopped.container.querySelector(
      '[data-slot="inspect-step-rail"] [data-key="final"]',
    ),
  ).toBeNull()
  await stopped.unmount()

  const unpublished = await render_(false)
  await expect
    .element(page.getByText(m.inspect_candidate_applied_unpublished()))
    .toBeVisible()
  expect(
    unpublished.container.querySelector(
      '[data-slot="inspect-candidate-halted"]',
    ),
  ).toBeNull()
  await unpublished.unmount()
})
