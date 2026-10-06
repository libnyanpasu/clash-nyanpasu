import { useSyncExternalStore, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import { m } from '@/paraglide/messages'
import type {
  ClashConnection_Serialize,
  ClosedConnection,
} from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import AllViewer, {
  mergeConnectionRows,
} from '../src/pages/(main)/main/connections/_modules/all-viewer'
import { mockActiveConnections } from '../src/pages/(main)/main/connections/_modules/mock-connections'
import type { ConnectionRow } from '../src/pages/(main)/main/connections/_modules/use-connection-rows'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Generated commands take the desktop IPC path that `mockIPC` serves.
beforeEach(() => vi.stubGlobal('isTauri', true))
afterEach(() => vi.unstubAllGlobals())

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ isMinimized: async () => false }),
}))

// The live rows come from the connection stream; the test publishes its
// frames directly.
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

const live = (id: string): ConnectionRow => {
  const [connection] = mockActiveConnections(0) as [ClashConnection_Serialize]
  return { ...connection, id, startMs: Date.parse(connection.start) }
}

test('merges live rows first, then closed rows newest first', () => {
  const rows = mergeConnectionRows(
    [live('x'), live('y')],
    [closed('b', 'b.example', 2_000), closed('a', 'a.example', 1_000)],
  )

  expect(rows.map((row) => row.key)).toEqual([
    'a:x',
    'a:y',
    'c:2000:b',
    'c:1000:a',
  ])
})

test('a closed record of a live id is left out', () => {
  const rows = mergeConnectionRows(
    [live('x')],
    [closed('x', 'reused.example', 2_000), closed('a', 'a.example', 1_000)],
  )

  expect(rows.map((row) => row.key)).toEqual(['a:x', 'c:1000:a'])
})

test('a live connection a search hides is not shown as closed', () => {
  // The search matches only the closed record, so no live row is passed.
  const rows = mergeConnectionRows(
    [],
    [closed('x', 'reused.example', 2_000), closed('a', 'a.example', 1_000)],
    [live('x')],
  )

  expect(rows.map((row) => row.key)).toEqual(['c:1000:a'])
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

const menuText = () =>
  document.querySelector('[role="menu"]')?.textContent ?? null

test('lists live and closed connections in one table', async ({
  onTestFinished,
}) => {
  const [connection] = mockActiveConnections(0) as [ClashConnection_Serialize]
  stream.publish({
    connections: [{ ...connection, downloadSpeed: 4096, uploadSpeed: 2048 }],
  })
  mockIPC(() => ({
    connections: [
      closed('b', 'newer.example', 2_000),
      closed('a', 'older.example', 1_000),
    ],
    next: null,
  }))

  const container = render(
    <AllViewer
      search=""
      selection={{ filters: [] }}
      settingsOpen={false}
      onSettingsOpenChange={() => {}}
    />,
    onTestFinished,
  )

  await expect.poll(() => container.querySelectorAll('tbody tr').length).toBe(3)
  const [liveRow, newer, older] = container.querySelectorAll('tbody tr')
  expect(liveRow.textContent).toContain('/s')
  expect(newer.textContent).toContain('newer.example')
  expect(newer.textContent).not.toContain('/s')
  expect(older.textContent).toContain('older.example')

  // The status dot is an image named by its status, not only a colour.
  const status = (row: Element) =>
    row.querySelector<HTMLElement>('[data-slot="connections-status-dot"]')!
  await expect.element(status(liveRow)).toHaveRole('img')
  await expect
    .element(status(liveRow))
    .toHaveAccessibleName(m.connections_tab_active())
  await expect
    .element(status(newer))
    .toHaveAccessibleName(m.connections_tab_closed())

  const headers = [...container.querySelectorAll('th')].map(
    (th) => th.textContent,
  )
  expect(headers).toContain(m.connections_column_status())
  expect(headers).toContain(m.connections_column_destination())

  await userEvent.click(liveRow.querySelector('td')!, { button: 'right' })
  await expect.poll(menuText).toContain(m.connections_close_connection())
  await userEvent.keyboard('{Escape}')
  await expect.poll(menuText).toBeNull()

  await userEvent.click(newer.querySelector('td')!, { button: 'right' })
  await expect.poll(menuText).toContain(m.connections_view_details())
  expect(menuText()).not.toContain(m.connections_close_connection())
})
