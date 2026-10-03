import { useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import ContextMenuProvider from '@/components/providers/context-menu-provider'
import parseTraffic from '@/utils/parse-traffic'
import type { ClashConnection_Serialize } from '@nyanpasu/rpc/types'
import { QueryClient } from '@tanstack/react-query'
import ActiveViewer from '../src/pages/(main)/main/connections/_modules/active-viewer'
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

function renderActiveViewer(onTestFinished: (fn: () => void) => void) {
  const queries = new QueryClient()
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
    queries.clear()
    localStorage.clear()
  })
  root.render(
    <QueryClientProvider client={queries}>
      <ContextMenuProvider>
        <ScrollArea className="h-96">
          <ActiveViewer
            search=""
            filters={[]}
            settingsOpen={false}
            onSettingsOpenChange={() => {}}
          />
        </ScrollArea>
      </ContextMenuProvider>
    </QueryClientProvider>,
  )
  return container
}

// Each sample arrives as new objects, as a deserialized frame does.
const sample = (
  connection: ClashConnection_Serialize,
  changes: Partial<ClashConnection_Serialize> = {},
) => ({ connections: [{ ...connection, ...changes }] })

const rowText = (container: HTMLElement) =>
  container.querySelector('tbody tr')?.textContent ?? ''

test('a row shows the traffic of each new sample', async ({
  onTestFinished,
}) => {
  const [connection] = mockActiveConnections(0) as [ClashConnection_Serialize]
  stream.publish(sample(connection, { download: 1024 }))

  const container = renderActiveViewer(onTestFinished)

  await expect
    .poll(() => rowText(container))
    .toContain(parseTraffic(1024).join(' '))

  stream.publish(sample(connection, { download: 5 * 1024 * 1024 }))

  await expect
    .poll(() => rowText(container))
    .toContain(parseTraffic(5 * 1024 * 1024).join(' '))
})

test('a row whose traffic did not change still moves its relative time on', async ({
  onTestFinished,
}) => {
  vi.useFakeTimers({ toFake: ['Date'] })
  onTestFinished(() => {
    vi.useRealTimers()
  })

  const now = Date.UTC(2026, 0, 1)
  vi.setSystemTime(now)

  const [connection] = mockActiveConnections(0) as [ClashConnection_Serialize]
  const started = {
    ...connection,
    start: new Date(now - 10_000).toISOString(),
  }
  stream.publish(sample(started))

  const container = renderActiveViewer(onTestFinished)

  await expect
    .poll(() => rowText(container))
    .toContain('less than a minute ago')

  vi.setSystemTime(now + 5 * 60_000)
  stream.publish(sample(started))

  await expect.poll(() => rowText(container)).toContain('5 minutes ago')
})
