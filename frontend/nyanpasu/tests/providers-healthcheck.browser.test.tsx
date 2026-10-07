import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { BlockTaskProvider } from '@/components/providers/block-task-provider'
import { InfoCard } from '@/pages/(main)/main/providers/proxies/_modules/info-card'
import { m } from '@/paraglide/messages'
import {
  useClashProxies,
  useClashProxiesProvider,
  type ClashProxiesProviderQueryItem,
} from '@nyanpasu/query'
import { QueryClient } from '@tanstack/react-query'
import { createTestRpc, rpcWrapper } from '../../query/tests/rpc-test-utils'

const notify = vi.hoisted(() => ({ message: vi.fn() }))
vi.mock('@/utils/notification', () => notify)

const provider: ClashProxiesProviderQueryItem = {
  name: 'sub',
  type: 'Proxy',
  vehicleType: 'HTTP',
  updatedAt: null,
  subscriptionInfo: null,
  proxyCount: 1,
}

// Keeps the providers and proxies queries mounted next to the card.
function Probe() {
  useClashProxiesProvider()
  useClashProxies()
  return null
}

async function setup(
  onTestFinished: (fn: () => Promise<void>) => void,
  healthcheck: () => unknown,
) {
  const calls = { providers: 0, proxies: 0 }
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity, gcTime: Infinity },
      mutations: { retry: false },
    },
  })
  const testRpc = createTestRpc({
    clash_api_get_providers_proxies: async () => {
      calls.providers += 1
      return {}
    },
    get_proxies: async () => {
      calls.proxies += 1
      return { global: null, groups: [], nodes: {} }
    },
    clash_api_healthcheck_proxy_provider: async () => healthcheck(),
  })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const view = await render(
    <Wrapper>
      <BlockTaskProvider>
        <Probe />
        <InfoCard data={provider} />
      </BlockTaskProvider>
    </Wrapper>,
  )
  onTestFinished(async () => {
    await view.unmount()
    await client.cancelQueries()
    client.clear()
    testRpc.rpc.dispose()
    vi.clearAllMocks()
  })
  await expect.poll(() => calls.providers + calls.proxies).toBe(2)
  return { view, calls, testRpc }
}

test('health check calls the provider command, then refetches providers and proxies', async ({
  onTestFinished,
}) => {
  const { view, calls, testRpc } = await setup(onTestFinished, () => ({
    status: 'committed',
    value: null,
    commits: [],
    notifications_pending: false,
  }))

  await view
    .getByRole('button', { name: m.providers_healthcheck_provider() })
    .click()

  await expect.poll(() => calls).toEqual({ providers: 2, proxies: 2 })
  expect(testRpc.invoke).toHaveBeenCalledWith(
    'clash_api_healthcheck_proxy_provider',
    { name: 'sub' },
  )
  expect(notify.message).not.toHaveBeenCalled()
})

test('a rejected health check reports the failure', async ({
  onTestFinished,
}) => {
  const { view } = await setup(onTestFinished, () => {
    throw new Error('boom')
  })

  await view
    .getByRole('button', { name: m.providers_healthcheck_provider() })
    .click()

  await expect
    .poll(() => notify.message.mock.calls[0]?.[0])
    .toBe(m.providers_healthcheck_failed_message({ name: 'sub' }))
})
