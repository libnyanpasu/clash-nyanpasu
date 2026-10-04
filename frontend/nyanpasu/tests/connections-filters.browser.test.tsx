import { useSyncExternalStore, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import type {
  ClashConnection_Serialize,
  ClosedConnection,
  ClosedCursor,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import ActiveViewer from '../src/pages/(main)/main/connections/_modules/active-viewer'
import ClosedViewer from '../src/pages/(main)/main/connections/_modules/closed-viewer'
import ConnectionsFilters from '../src/pages/(main)/main/connections/_modules/connections-filters'
import { mockActiveConnections } from '../src/pages/(main)/main/connections/_modules/mock-connections'
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

type RpcCall = { method: string; params: Record<string, unknown> }

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

const rowCount = (container: HTMLElement) =>
  container.querySelectorAll('tbody tr').length

const hasRow = (container: HTMLElement, text: string) =>
  [...container.querySelectorAll('tbody tr')].some((tr) =>
    tr.textContent?.includes(text),
  )

const twoConnections = () => {
  const [first, second] = mockActiveConnections(0) as [
    ClashConnection_Serialize,
    ClashConnection_Serialize,
  ]
  const a = {
    ...first,
    id: 'a',
    metadata: { ...first.metadata!, host: 'a.example' },
  }
  const b = {
    ...second,
    id: 'b',
    metadata: { ...second.metadata!, host: 'b.example' },
  }
  return [a, b]
}

test('a filtered active view lists only the connections the backend matched', async ({
  onTestFinished,
}) => {
  const calls: RpcCall[] = []
  mockIPC((_, args) => {
    const call = args as RpcCall
    calls.push(call)
    if (call.method === 'query_traffic_active_connection_ids') {
      return ['a']
    }
    throw new Error(`unexpected ${call.method}`)
  })
  stream.publish({ connections: twoConnections() })

  const container = render(
    <ActiveViewer
      search=""
      filters={[{ d: 'process', v: 'curl' }]}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => hasRow(container, 'a.example')).toBe(true)
  expect(rowCount(container)).toBe(1)
  expect(hasRow(container, 'b.example')).toBe(false)
  expect(
    calls.find((call) => call.method === 'query_traffic_active_connection_ids')!
      .params.filters,
  ).toEqual([{ dimension: 'process', value: 'curl' }])
})

test('a filtered active view explains unavailable ids and shows no rows', async ({
  onTestFinished,
}) => {
  const asked = vi.fn()
  mockIPC((_, args) => {
    if ((args as RpcCall).method === 'query_traffic_active_connection_ids') {
      asked()
    }
    throw new Error('traffic recording is unavailable')
  })
  stream.publish({ connections: twoConnections() })

  const container = render(
    <ActiveViewer
      search=""
      filters={[{ d: 'process', v: 'curl' }]}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => asked.mock.calls.length).toBeGreaterThan(0)
  await expect
    .poll(() => container.textContent)
    .toContain(m.connections_active_unavailable())
  expect(container.textContent).not.toContain(m.connections_empty_message())
  expect(hasRow(container, 'a.example')).toBe(false)
  expect(hasRow(container, 'b.example')).toBe(false)
})

const closed = (id: string, target: string): ClosedConnection => ({
  id,
  started_at: 1_000,
  first_seen_at: 1_000,
  closed_at: 2_000,
  bytes: { upload: 1024, download: 2048 },
  dimensions: {
    process: '/usr/bin/curl',
    source: '192.168.1.2',
    target,
    protocol: 'tcp',
    rule: { kind: 'Match', payload: '' },
    chains: ['Node-A', 'Proxy'],
  },
})

test('a filtered closed view asks for the selection and pages past empty pages', async ({
  onTestFinished,
}) => {
  const cursor: ClosedCursor = { closed_at: 5_000, id: 'c1' }
  const calls: RpcCall['params'][] = []
  mockIPC((_, args) => {
    const { params } = args as RpcCall
    calls.push(params)
    return params.before === null
      ? { connections: [], next: cursor }
      : { connections: [closed('a', 'matched.example')], next: null }
  })

  const container = render(
    <ClosedViewer
      search=""
      selection={{ range: 'last_hour', filters: [{ d: 'process', v: 'curl' }] }}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => hasRow(container, 'matched.example')).toBe(true)
  expect(calls[0]).toMatchObject({
    range: 'last_hour',
    filters: [{ dimension: 'process', value: 'curl' }],
    before: null,
  })
  expect(calls[1]).toMatchObject({ before: cursor })
})

test('a filtered closed view explains an unavailable history', async ({
  onTestFinished,
}) => {
  mockIPC(() => {
    throw new Error('traffic recording is unavailable')
  })

  const container = render(
    <ClosedViewer
      search=""
      selection={{ filters: [{ d: 'process', v: 'curl' }] }}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect
    .poll(() => container.textContent)
    .toContain(m.connections_closed_unavailable())
})

test('the selection shows as chips that remove one condition or all', async ({
  onTestFinished,
}) => {
  mockIPC(() => {
    throw new Error('unavailable')
  })
  const onSelectionChange = vi.fn()

  const container = render(
    <ConnectionsFilters
      selection={{
        range: 'last_hour',
        filters: [
          { d: 'process', v: '/usr/bin/curl' },
          { d: 'target', v: 'example.com' },
        ],
      }}
      onSelectionChange={onSelectionChange}
    />,
    onTestFinished,
  )

  const chips = () =>
    [...container.querySelectorAll('[data-slot="traffic-filter-chip"]')].map(
      (chip) => chip.textContent,
    )

  await expect.poll(chips).toHaveLength(3)
  expect(chips()).toEqual([
    `${m.traffic_dimension_process()}: curl`,
    `${m.traffic_dimension_target()}: example.com`,
    `${m.traffic_range_label()}: ${m.traffic_range_last_hour()}`,
  ])

  container
    .querySelector<HTMLButtonElement>(
      `[aria-label="${m.traffic_filter_remove({ filter: `${m.traffic_dimension_process()}: curl` })}"]`,
    )!
    .click()
  expect(onSelectionChange).toHaveBeenLastCalledWith({
    range: 'last_hour',
    filters: [{ d: 'target', v: 'example.com' }],
  })

  container
    .querySelector<HTMLButtonElement>(
      `[aria-label="${m.traffic_filter_remove({ filter: `${m.traffic_range_label()}: ${m.traffic_range_last_hour()}` })}"]`,
    )!
    .click()
  expect(onSelectionChange).toHaveBeenLastCalledWith({
    range: undefined,
    filters: [
      { d: 'process', v: '/usr/bin/curl' },
      { d: 'target', v: 'example.com' },
    ],
  })

  ;[...container.querySelectorAll('button')]
    .find((button) => button.textContent === m.traffic_filter_clear_all())!
    .click()
  expect(onSelectionChange).toHaveBeenLastCalledWith({
    range: undefined,
    filters: [],
  })
})
