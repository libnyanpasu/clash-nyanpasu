import { StrictMode } from 'react'
import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useRouteEgressProbe } from '../src/ipc/use-route-egress-probe'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

type Route = {
  systemProxy: boolean
  tun: boolean
  capture: boolean
}

const appSettings = (allowed: boolean, { systemProxy }: Route) => ({
  enable_local_ip_probe: allowed,
  enable_system_proxy: systemProxy,
})

const clashSettings = ({ tun, capture }: Route) => ({
  enable_tun_mode: tun,
  transparent_proxy: { mode: capture ? 'tproxy' : 'disabled', local: true },
})

const DIRECT: Route = { systemProxy: false, tun: false, capture: false }

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  allowed: boolean,
  route: Route = DIRECT,
) {
  const testRpc = createTestRpc({
    get_app_settings: () => appSettings(allowed, route),
    get_clash_settings: () => clashSettings(route),
    probe_route_egress: () => ({
      kind: 'probed',
      method: 'direct',
      ipv4: null,
      ipv6: null,
    }),
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  // Exercise StrictMode with settings already cached, as on a return visit.
  client.setQueryData(['getAppSettings'], appSettings(allowed, route))
  client.setQueryData(['getClashSettings'], clashSettings(route))
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook(() => useRouteEgressProbe(), {
    wrapper: ({ children }) => (
      <StrictMode>
        <Wrapper>{children}</Wrapper>
      </StrictMode>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
  })
  const probes = () =>
    testRpc.invoke.mock.calls.filter(
      ([method]) => method === 'probe_route_egress',
    ).length
  return { hook, client, probes, testRpc }
}

test.for<Route>([
  DIRECT,
  { ...DIRECT, systemProxy: true },
  { ...DIRECT, tun: true },
  { ...DIRECT, capture: true },
])(
  'probes once whatever the route is: %j',
  async (route, { onTestFinished }) => {
    const { hook, client, probes } = await setup(onTestFinished, true, route)
    await expect.poll(() => hook.result.current.isSuccess).toBe(true)
    expect(probes()).toBe(1)

    await hook.act(async () => {
      window.dispatchEvent(new Event('focus'))
      window.dispatchEvent(new Event('online'))
      await client.refetchQueries({ queryKey: ['getAppSettings'] })
      await client.refetchQueries({ queryKey: ['getClashSettings'] })
    })
    expect(probes()).toBe(1)
  },
)

test('does not probe while detection is off, and probes once it is turned on', async ({
  onTestFinished,
}) => {
  const { hook, client, probes } = await setup(onTestFinished, false)
  expect(hook.result.current.isIdle).toBe(true)
  expect(probes()).toBe(0)

  await hook.act(() =>
    client.setQueryData(['getAppSettings'], appSettings(true, DIRECT)),
  )
  await expect.poll(probes).toBe(1)
})

test('probes again when the system proxy, TUN or local capture changes', async ({
  onTestFinished,
}) => {
  const { hook, client, probes } = await setup(onTestFinished, true)
  await expect.poll(probes).toBe(1)

  await hook.act(() =>
    client.setQueryData(
      ['getAppSettings'],
      appSettings(true, { ...DIRECT, systemProxy: true }),
    ),
  )
  await expect.poll(probes).toBe(2)

  await hook.act(() =>
    client.setQueryData(
      ['getClashSettings'],
      clashSettings({ ...DIRECT, tun: true }),
    ),
  )
  await expect.poll(probes).toBe(3)

  await hook.act(() =>
    client.setQueryData(
      ['getClashSettings'],
      clashSettings({ ...DIRECT, tun: true, capture: true }),
    ),
  )
  await expect.poll(probes).toBe(4)
})

test('a failed probe is not retried automatically', async ({
  onTestFinished,
}) => {
  const { hook, client, testRpc, probes } = await setup(onTestFinished, false)
  testRpc.invoke.mockRejectedValueOnce(new Error('probe unavailable'))
  await hook.act(() =>
    client.setQueryData(['getAppSettings'], appSettings(true, DIRECT)),
  )
  await expect.poll(() => hook.result.current.isError).toBe(true)
  expect(probes()).toBe(1)

  await hook.rerender()
  expect(probes()).toBe(1)
})
