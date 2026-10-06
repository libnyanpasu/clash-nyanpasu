import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import type {
  ClosedConnection,
  ClosedCursor,
  TrafficFilter,
  TrafficRange,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import ClosedViewer from '../src/pages/(main)/main/connections/_modules/closed-viewer'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

const UNFILTERED = { filters: [] }

const closed = (
  id: string,
  target: string,
  closedAt: number,
): ClosedConnection => ({
  id,
  started_at: closedAt - 1_000,
  first_seen_at: closedAt - 1_000,
  closed_at: closedAt,
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

const headers = (container: HTMLElement) =>
  [...container.querySelectorAll('th')].map((th) => th.textContent)

test('lists closed connections and loads older pages at the end', async ({
  onTestFinished,
}) => {
  const newest = closed('b', 'example.com', 2_000)
  const requested: Array<ClosedCursor | null> = []
  mockIPC((command, args) => {
    expect(command).toBe('call_rpc')
    expect((args as { method: string }).method).toBe(
      'query_traffic_closed_connections',
    )
    const { range, filters, before, limit } = (
      args as {
        params: {
          range: TrafficRange
          filters: TrafficFilter[]
          before: ClosedCursor | null
          limit: number
        }
      }
    ).params
    expect({ range, filters, limit }).toEqual({
      range: 'all',
      filters: [],
      limit: 200,
    })
    requested.push(before)
    return before === null
      ? {
          connections: [newest],
          next: { closed_at: newest.closed_at, id: newest.id },
        }
      : { connections: [closed('a', 'older.example', 1_000)], next: null }
  })

  const container = render(
    <ClosedViewer
      search=""
      selection={UNFILTERED}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.textContent).toContain('older.example')
  expect(container.textContent).toContain('example.com')
  // The table shows the process name; the path stays in the title.
  expect(container.textContent).toContain('curl')
  expect(container.textContent).not.toContain('/usr/bin/curl')
  expect(requested.slice(0, 2)).toEqual([
    null,
    { closed_at: newest.closed_at, id: newest.id },
  ])
  expect(headers(container)).toContain(m.connections_column_closed_time())
})

test('a search keeps loading older pages until it finds a match', async ({
  onTestFinished,
}) => {
  const newest = closed('b', 'example.com', 2_000)
  mockIPC((_, args) => {
    const { before } = (args as { params: { before: ClosedCursor | null } })
      .params
    return before === null
      ? {
          connections: [newest],
          next: { closed_at: newest.closed_at, id: newest.id },
        }
      : { connections: [closed('a', 'older.example', 1_000)], next: null }
  })

  const container = render(
    <ClosedViewer
      search="older"
      selection={UNFILTERED}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.textContent).toContain('older.example')
  expect(container.querySelectorAll('tbody tr')).toHaveLength(1)
})

test('column settings hide a column and remember it', async ({
  onTestFinished,
}) => {
  mockIPC(() => ({
    connections: [closed('a', 'example.com', 1_000)],
    next: null,
  }))

  const container = render(
    <ClosedViewer
      search=""
      selection={UNFILTERED}
      settingsOpen
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect
    .poll(() => headers(container))
    .toContain(m.connections_column_process())

  const item = [
    ...document.querySelectorAll(
      '[data-slot="connections-column-settings-item"]',
    ),
  ].find((node) => node.textContent === m.connections_column_process())
  item!.querySelector<HTMLButtonElement>('[role="switch"]')!.click()

  await expect
    .poll(() => headers(container))
    .not.toContain(m.connections_column_process())
  expect(
    JSON.parse(localStorage.getItem('connections-columns-closed')!).visibility,
  ).toEqual({ Process: false })
})

test('an unavailable traffic history explains the empty table', async ({
  onTestFinished,
}) => {
  mockIPC(() => {
    throw new Error('traffic recording is unavailable')
  })

  const container = render(
    <ClosedViewer
      search=""
      selection={UNFILTERED}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect
    .poll(() => container.textContent)
    .toContain(m.connections_closed_unavailable())
})

test('the mock connections setting replaces the traffic history', async ({
  onTestFinished,
}) => {
  localStorage.setItem('debug-mock-connections', 'true')
  mockIPC(() => ({ connections: [], next: null }))

  const container = render(
    <ClosedViewer
      search=""
      selection={UNFILTERED}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect
    .poll(() => container.querySelectorAll('tbody tr').length)
    .toBeGreaterThan(0)
})
