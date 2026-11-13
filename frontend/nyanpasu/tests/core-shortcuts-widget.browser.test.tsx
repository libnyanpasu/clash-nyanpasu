import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { createElement, type ComponentProps, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import { DashboardProvider } from '@/components/widgets/provider'
import { CoreShortcutsWidget } from '@/components/widgets/widget-shortcut'
import { m } from '@/paraglide/messages'
import { DndContext } from '@dnd-kit/core'
import { KvStorageProvider, RpcProvider } from '@nyanpasu/query/provider'
import {
  createIconTransport,
  createRpcClient,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

vi.mock('@tanstack/react-router', async (importOriginal) => {
  const original =
    await importOriginal<typeof import('@tanstack/react-router')>()
  return {
    ...original,
    Link: ({
      children,
      ...props
    }: ComponentProps<'a'> & { children: ReactNode }) =>
      createElement('a', props, children),
  }
})

// Reads that carry the daemon's directories, the core's controller endpoint
// and failure text, or any other diagnostic text: an ordinary dashboard must
// not issue them.
const PROTECTED_READS = [
  'get_core_status',
  'status_service',
  'get_configuration_status',
]

const events: RpcEventTransport = {
  listen: async () => () => {},
  once: async () => () => {},
  emit: async () => {},
  listenResync: () => () => {},
  dispose: () => {},
}

function mountWidget(
  onTestFinished: (fn: () => void) => void,
  summaries: { core: unknown; service: unknown },
) {
  const invoke = vi.fn(async (method: string) => {
    if (PROTECTED_READS.includes(method)) {
      throw new Error(`Protected read on ordinary bootstrap: ${method}`)
    }
    switch (method) {
      case 'get_core_status_summary':
        return summaries.core
      case 'get_service_status_summary':
        return summaries.service
      case 'get_storage_item':
        return { status: 'ok', data: { value: null, revision: 0 } }
      default:
        return null
    }
  })
  const rpc = createRpcClient({
    icons: createIconTransport(false),
    commands: {
      invoke: async <T,>(method: string) => (await invoke(method)) as T,
    },
    events,
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  root.render(
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>
        <KvStorageProvider>
          <DashboardProvider>
            <TooltipProvider>
              <DndContext>
                <DndGridProvider
                  value={{
                    displayItems: [{ id: 'core', x: 0, y: 0, w: 4, h: 3 }],
                    getItemRect: () => ({
                      left: 0,
                      top: 0,
                      width: 320,
                      height: 220,
                    }),
                    dropInfoMap: {},
                    activeItemId: null,
                    resizingItemId: null,
                    disabled: true,
                    sourceOnly: false,
                    dragIdPrefix: '',
                    isOverlay: false,
                    constraintsMapRef: { current: {} },
                    onResizeStart: () => {},
                    onResizeMove: () => {},
                    onResizeEnd: () => {},
                  }}
                >
                  <CoreShortcutsWidget id="core" onCloseClick={() => {}} />
                </DndGridProvider>
              </DndContext>
            </TooltipProvider>
          </DashboardProvider>
        </KvStorageProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )
  onTestFinished(() => {
    root.unmount()
    client.clear()
    container.remove()
    rpc.dispose()
  })
  return {
    invoke,
    badge: () =>
      container
        .querySelector('[data-slot="core-status-badge"]')
        ?.textContent?.trim(),
  }
}

const service = (status: string, server_reported = false) => ({
  name: 'nyanpasu-service',
  version: '2.0.3',
  status,
  server_reported,
  phase: status === 'running' ? 'ready' : 'daemon_stopped',
  compat: { kind: 'compatible', server_version: '2.0.3' },
  restart_attempts: 0,
})

const core = (host: 'local' | 'service', state: unknown) => ({
  controller: 'Http',
  host,
  connectivity: { kind: 'connected' },
  generation: 1,
  state,
  state_changed_at: 0,
  revision: null,
  healthy: true,
})

test('the dashboard core widget bootstraps from the safe summaries only', async ({
  onTestFinished,
}) => {
  const widget = mountWidget(onTestFinished, {
    core: core('local', { Running: { epoch: 1 } }),
    service: service('not_installed'),
  })

  await expect
    .poll(widget.badge)
    .toBe(m.dashboard_widget_core_status_running_by_child_process())
  const methods = widget.invoke.mock.calls.map(([method]) => method)
  expect(methods).toContain('get_core_status_summary')
  expect(methods).toContain('get_service_status_summary')
  for (const protectedRead of PROTECTED_READS) {
    expect(methods).not.toContain(protectedRead)
  }
})

test('a stopped core is worded from the state and the flagged reason, never its text', async ({
  onTestFinished,
}) => {
  const widget = mountWidget(onTestFinished, {
    core: core('service', { Stopped: { has_reason: true } }),
    service: service('running', true),
  })

  await expect
    .poll(widget.badge)
    // The marquee may repeat the text to loop it.
    .toContain(
      `${m.dashboard_widget_core_stopped_by_service_unknown()} ${m.dashboard_widget_core_service_running()}`,
    )
  expect(
    widget.invoke.mock.calls.some(([method]) =>
      PROTECTED_READS.includes(method),
    ),
  ).toBe(false)
})
