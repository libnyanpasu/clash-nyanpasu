import { StrictMode } from 'react'
import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useLocalIpProbe } from '../src/ipc/use-local-ip-probe'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  allowed: boolean,
  tun: boolean,
  enabled = true,
) {
  const testRpc = createTestRpc({
    get_app_config: () => ({ enable_local_ip_probe: allowed }),
    get_clash_config: () => ({ enable_tun_mode: tun }),
    probe_direct_egress: () => ({
      kind: 'probed',
      ipv4: '203.0.113.7',
      ipv6: null,
    }),
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  // Exercise StrictMode with settings already cached, as on a return visit.
  client.setQueryData(['getAppConfig'], { enable_local_ip_probe: allowed })
  client.setQueryData(['getClashConfig'], { enable_tun_mode: tun })
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const hook = await renderHook((props) => useLocalIpProbe(props?.enabled), {
    initialProps: { enabled },
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
      ([method]) => method === 'probe_direct_egress',
    ).length
  return { hook, client, probes, testRpc }
}

test('probes once per eligible visit without polling or focus/reconnect probes', async ({
  onTestFinished,
}) => {
  const { hook, client, probes } = await setup(onTestFinished, true, false)
  await expect.poll(() => hook.result.current.isSuccess).toBe(true)
  expect(probes()).toBe(1)
  await hook.rerender({ enabled: true })
  await hook.act(async () => {
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('online'))
    await client.refetchQueries({ queryKey: ['getAppConfig'] })
    await client.refetchQueries({ queryKey: ['getClashConfig'] })
  })
  expect(probes()).toBe(1)
  await hook.rerender({ enabled: false })
  await hook.rerender({ enabled: true })
  await expect.poll(probes).toBe(2)
})

test.for([
  { allowed: false, tun: false, enabled: true },
  { allowed: true, tun: true, enabled: true },
  { allowed: true, tun: false, enabled: false },
])(
  'does not probe when ineligible: %j',
  async ({ allowed, tun, enabled }, { onTestFinished }) => {
    const { hook, probes } = await setup(onTestFinished, allowed, tun, enabled)
    expect(hook.result.current.isIdle).toBe(true)
    expect(probes()).toBe(0)
  },
)

test('enabling permission triggers a probe and failures do not retry automatically', async ({
  onTestFinished,
}) => {
  const { hook, client, testRpc, probes } = await setup(
    onTestFinished,
    false,
    false,
  )
  testRpc.invoke.mockRejectedValueOnce(new Error('probe unavailable'))
  await hook.act(() =>
    client.setQueryData(['getAppConfig'], { enable_local_ip_probe: true }),
  )
  await expect.poll(() => hook.result.current.isError).toBe(true)
  expect(probes()).toBe(1)
  await hook.rerender({ enabled: true })
  expect(probes()).toBe(1)
})
