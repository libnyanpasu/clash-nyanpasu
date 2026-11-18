import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useCoreStatus } from '../src/hooks/use-core-status'
import { useServiceStatus } from '../src/ipc/use-service-status'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const summary = {
  controller: 'UnixSocket',
  host: 'service',
  connectivity: { kind: 'connected' },
  generation: 3,
  state: { Running: { epoch: 2 } },
  state_changed_at: 42,
  revision: { epoch: 1, generation: 5 },
  healthy: true,
}

const full = {
  controller: { UnixSocket: '/run/user/1000/clash-sentinel.sock' },
  host: 'service',
  connectivity: { kind: 'connected' },
  generation: 3,
  state: { Stopped: { reason: 'sentinel stop reason' } },
  state_changed_at: 42,
  revision: null,
  healthy: false,
}

function setup() {
  const testRpc = createTestRpc({
    get_core_status_summary: () => summary,
    get_core_status: () => full,
    get_service_status_summary: () => ({
      name: 'test-service',
      version: '2.0.2',
      status: 'running',
      server_reported: true,
      phase: 'ready',
      compat: { kind: 'compatible', server_version: '2.0.2' },
      restart_attempts: 0,
    }),
  })
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return { testRpc, client, wrapper: rpcWrapper(testRpc.rpc, client) }
}

test('the ordinary core and service status read only the safe summaries', async ({
  onTestFinished,
}) => {
  const { testRpc, client, wrapper } = setup()
  const core = await renderHook(useCoreStatus, { wrapper })
  const service = await renderHook(useServiceStatus, { wrapper })
  onTestFinished(async () => {
    await core.unmount()
    await service.unmount()
    client.clear()
    testRpc.rpc.dispose()
  })

  await expect
    .poll(() => core.result.current.data)
    .toEqual({
      status: 'Running',
      startAt: 42,
      type: 'service',
      controller: 'UnixSocket',
    })
  await expect
    .poll(() => service.result.current.data?.server_reported)
    .toBe(true)
  const methods = testRpc.invoke.mock.calls.map(([method]) => method)
  expect(methods).toContain('get_core_status_summary')
  expect(methods).toContain('get_service_status_summary')
  expect(methods).not.toContain('get_core_status')
  expect(methods).not.toContain('status_service')
})
