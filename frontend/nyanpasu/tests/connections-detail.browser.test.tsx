import { useSyncExternalStore, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import type {
  ClashConnection_Serialize,
  ClosedConnection,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import ActiveViewer from '../src/pages/(main)/main/connections/_modules/active-viewer'
import ClosedViewer from '../src/pages/(main)/main/connections/_modules/closed-viewer'
import { mockActiveConnections } from '../src/pages/(main)/main/connections/_modules/mock-connections'
import {
  closedConnectionDetail,
  ConnectionDetailModal,
} from '../src/pages/(main)/main/connections/_modules/table-row'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

// The active view reads the connection stream from a provider; the test
// publishes its frames directly.
const stream = vi.hoisted(() => {
  let frame: { connections: unknown[] } | null = null
  const listeners = new Set<() => void>()
  return {
    get: () => frame,
    publish(next: { connections: unknown[] }) {
      frame = next
      listeners.forEach((listener) => listener())
    },
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
  }
})

vi.mock('@nyanpasu/query', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nyanpasu/query')>()),
  useClashConnectionDetails: () => {
    const data = useSyncExternalStore(stream.subscribe, stream.get)
    return { data, isLoading: data === null }
  },
}))

function render(
  node: ReactNode,
  onTestFinished: (fn: () => void) => void,
): HTMLElement {
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    clearMocks()
    localStorage.clear()
  })
  root.render(
    <QueryClientProvider client={queries}>
      <ContextMenuProvider>
        <ScrollArea className="h-96">{node}</ScrollArea>
      </ContextMenuProvider>
    </QueryClientProvider>,
  )
  return container
}

const doubleClick = (element: Element) =>
  element.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))

const dialog = () => document.querySelector('[role="dialog"]')

const hasCloseButton = () =>
  [...(dialog()?.querySelectorAll('button') ?? [])].some(
    (button) => button.textContent === m.connections_close_connection(),
  )

test('double-clicking a closed connection shows its details', async ({
  onTestFinished,
}) => {
  const connection: ClosedConnection = {
    id: 'a',
    started_at: 1_000,
    first_seen_at: 1_000,
    closed_at: 2_000,
    bytes: { upload: 1024, download: 2048 },
    dimensions: {
      process: '/usr/bin/curl',
      source: '192.168.1.2',
      target: 'example.com',
      protocol: 'tcp',
      rule: { kind: 'Match', payload: '' },
      chains: ['Node-A', 'Proxy'],
    },
  }
  mockIPC(() => ({ connections: [connection], next: null }))

  const container = render(
    <ClosedViewer
      search=""
      selection={{ filters: [] }}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.querySelectorAll('tbody tr').length).toBe(1)
  doubleClick(container.querySelector('tbody tr')!)

  await expect.poll(() => dialog()?.textContent).toContain('/usr/bin/curl')
  expect(dialog()!.textContent).toContain(m.connections_tab_closed())
  expect(dialog()!.textContent).toContain(m.connections_column_closed_time())
  expect(hasCloseButton()).toBe(false)
})

test('the details of an active connection stay open after it closes', async ({
  onTestFinished,
}) => {
  const [connection, other] = mockActiveConnections(0) as [
    ClashConnection_Serialize,
    ClashConnection_Serialize,
  ]
  stream.publish({ connections: [connection, other] })

  const container = render(
    <ActiveViewer
      search=""
      filters={[]}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.querySelectorAll('tbody tr').length).toBe(2)
  const row = [...container.querySelectorAll('tbody tr')].find((tr) =>
    tr.textContent?.includes(connection.metadata!.host!),
  )!
  doubleClick(row)

  await expect.poll(() => dialog()?.textContent).toContain(connection.id)
  expect(dialog()!.textContent).not.toContain(m.connections_tab_closed())
  expect(hasCloseButton()).toBe(true)

  stream.publish({ connections: [other] })

  await expect
    .poll(() => dialog()?.textContent)
    .toContain(m.connections_tab_closed())
  expect(dialog()!.textContent).toContain(connection.id)
  expect(hasCloseButton()).toBe(false)
})

const closedMatch: ClosedConnection = {
  id: 'm',
  started_at: 1_000,
  first_seen_at: 1_000,
  closed_at: 2_000,
  bytes: { upload: 0, download: 0 },
  dimensions: {
    process: '',
    source: '192.168.1.2',
    target: 'example.com',
    protocol: 'tcp',
    rule: { kind: 'Match', payload: '' },
    chains: ['DIRECT'],
  },
}

const buttonNamed = (name: string) =>
  [...(dialog()?.querySelectorAll('button') ?? [])].find(
    (button) => button.textContent === name,
  )

test('the details close and jump to the rule and its usage with the detail', async ({
  onTestFinished,
}) => {
  // The dialog closes before the page changes, so it does not stay over it.
  const order: string[] = []
  const onClose = vi.fn(() => order.push('close'))
  const onLocateRule = vi.fn(() => order.push('locate'))
  const onViewRuleUsage = vi.fn(() => order.push('usage'))

  render(
    <ConnectionDetailModal
      detail={closedConnectionDetail(closedMatch, '2000:m')}
      onClose={onClose}
      onLocateRule={onLocateRule}
      onViewRuleUsage={onViewRuleUsage}
    />,
    onTestFinished,
  )

  await expect
    .poll(() => buttonNamed(m.connections_locate_rule()))
    .toBeDefined()

  buttonNamed(m.connections_locate_rule())!.click()
  buttonNamed(m.connections_view_rule_usage())!.click()

  expect(onLocateRule).toHaveBeenCalledWith(
    expect.objectContaining({ ruleLabel: 'Match', rowId: '2000:m' }),
  )
  expect(onViewRuleUsage).toHaveBeenCalledWith(
    expect.objectContaining({ ruleLabel: 'Match', rowId: '2000:m' }),
  )
  expect(order).toEqual(['close', 'locate', 'close', 'usage'])
})

test('the details offer no jumps without their callbacks', async ({
  onTestFinished,
}) => {
  render(
    <ConnectionDetailModal
      detail={closedConnectionDetail(closedMatch, '2000:m')}
      onClose={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => dialog()).not.toBeNull()
  expect(buttonNamed(m.connections_locate_rule())).toBeUndefined()
  expect(buttonNamed(m.connections_view_rule_usage())).toBeUndefined()
})
