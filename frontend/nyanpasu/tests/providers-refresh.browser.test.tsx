import { Profiler } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { useClashProxiesProvider } from '@nyanpasu/query'
import { MutationProvider } from '@nyanpasu/query/provider'
import { QueryClient } from '@tanstack/react-query'
import { emit } from '@tauri-apps/api/event'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { TestQueryProvider as QueryClientProvider } from './query-provider'

// Client construction selects the desktop event adapter for this test.
vi.hoisted(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {}, isTauri: true })
})

const settle = () => new Promise((resolve) => setTimeout(resolve, 300))

// Reads the providers the way the providers pages do.
function ProvidersProbe() {
  const { data } = useClashProxiesProvider()

  return (
    <ul>
      {Object.values(data ?? {}).map((provider) => (
        <li key={provider.name}>
          {provider.name} {provider.proxyCount}
        </li>
      ))}
    </ul>
  )
}

test('a proxies event refreshes only mounted provider views, without re-rendering unchanged ones', async ({
  onTestFinished,
}) => {
  let providerFetches = 0
  mockIPC(
    (wireCommand, args) => {
      expect(wireCommand).toBe('call_rpc')
      const { method } = args as { method: string }
      if (method !== 'clash_api_get_providers_proxies') return null
      providerFetches += 1
      return {
        sub: {
          name: 'sub',
          type: 'Proxy',
          vehicleType: 'HTTP',
          updatedAt: '2026-09-30T00:00:00Z',
          // Only the delay history changes between fetches.
          proxies: [
            {
              name: 'a',
              type: 'Shadowsocks',
              history: [{ delay: providerFetches }],
            },
          ],
        },
      }
    },
    { shouldMockEvents: true },
  )
  const queries = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  let commits = 0
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(async () => {
    root.unmount()
    container.remove()
    queries.clear()
    // Resolve provider subscription cleanup before clearing the native adapter.
    await Promise.resolve()
    clearMocks()
  })
  const render = (mounted: boolean) =>
    root.render(
      <QueryClientProvider client={queries}>
        <MutationProvider>
          <Profiler id="providers" onRender={() => (commits += 1)}>
            {mounted ? <ProvidersProbe /> : <div data-slot="other-page" />}
          </Profiler>
        </MutationProvider>
      </QueryClientProvider>,
    )

  render(true)
  await expect.poll(() => container.textContent).toBe('sub 1')
  await settle()

  commits = 0
  await emit('nyanpasu://mutation', 'proxies')
  await expect.poll(() => providerFetches).toBe(2)
  await settle()
  expect(commits).toBe(0)

  render(false)
  await expect
    .poll(() => container.querySelector('[data-slot="other-page"]'))
    .toBeTruthy()
  await emit('nyanpasu://mutation', 'proxies')
  await emit('nyanpasu://mutation', 'proxies')
  await settle()
  expect(providerFetches).toBe(2)
})
