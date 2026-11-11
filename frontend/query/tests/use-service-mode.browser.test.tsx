import { expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import type { MutationOutcome } from '@nyanpasu/rpc/types'
import { MutationCache, QueryClient } from '@tanstack/react-query'
import { useServiceMode } from '../src/hooks/use-service-mode'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const degraded: MutationOutcome<null> = {
  status: 'committed_degraded',
  value: null,
  commits: [
    {
      operation_id: 'service-mode-1',
      domain: 'application',
      source_version: 2,
      runtime: 'applied',
    },
  ],
  notifications_pending: true,
  degradations: [],
}

test('service mode mutation keeps the full outcome', async ({
  onTestFinished,
}) => {
  let enabled = false
  const settled: unknown[] = []
  const testRpc = createTestRpc({
    get_app_config: () => ({ enable_service_mode: enabled }),
    set_service_mode: (params) => {
      enabled = params?.enabled as boolean
      return degraded
    },
  })
  const client = new QueryClient({
    mutationCache: new MutationCache({
      onSuccess: (data) => {
        settled.push(data)
      },
    }),
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const hook = await renderHook(useServiceMode, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.value).toBe(false)

  let outcome: unknown
  await hook.act(async () => {
    outcome = await hook.result.current.mutate(true)
  })

  expect(outcome).toEqual(degraded)
  expect(settled).toEqual([degraded])
  expect(hook.result.current.value).toBe(true)
  expect(testRpc.invoke).toHaveBeenCalledWith('set_service_mode', {
    enabled: true,
  })
  expect(client.getMutationCache().getAll()[0].options.mutationKey).toEqual([
    'setServiceMode',
  ])
  expect(
    testRpc.invoke.mock.calls.some(([method]) => method === 'patch_app_config'),
  ).toBe(false)
})

test('service mode mutation waits for config readback', async ({
  onTestFinished,
}) => {
  let enabled = false
  let reads = 0
  let releaseReadback = () => {}
  const readback = new Promise<void>((resolve) => {
    releaseReadback = resolve
  })
  const testRpc = createTestRpc({
    get_app_config: async () => {
      if (reads++ > 0) await readback
      return { enable_service_mode: enabled }
    },
    set_service_mode: (params) => {
      enabled = params?.enabled as boolean
      return degraded
    },
  })
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const hook = await renderHook(useServiceMode, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    releaseReadback()
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.value).toBe(false)

  let resolved = false
  const mutation = hook.result.current.mutate(true).then((outcome) => {
    resolved = true
    return outcome
  })
  await expect.poll(() => reads).toBe(2)
  await expect.poll(() => hook.result.current.isPending).toBe(true)
  expect(resolved).toBe(false)
  expect(hook.result.current.value).toBe(false)

  releaseReadback()
  await hook.act(async () => {
    expect(await mutation).toEqual(degraded)
  })
  expect(hook.result.current.value).toBe(true)
  expect(hook.result.current.isPending).toBe(false)
})
