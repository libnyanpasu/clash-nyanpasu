import { expect, test, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { RpcFailure } from '@nyanpasu/rpc'
import type {
  ClashConfig,
  MutationOutcome,
  NyanpasuAppConfig_Serialize,
} from '@nyanpasu/rpc/types'
import { MutationCache, QueryClient } from '@tanstack/react-query'
import {
  useClashSetting,
  useClashSettings,
  useSetting,
  useSettings,
} from '../src/ipc/use-settings'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

const appConfig = {
  theme_mode: 'dark',
  core: 'mihomo',
  enable_service_mode: false,
  hotkeys: ['toggle_tun_mode,Control+Q'],
} satisfies Partial<NyanpasuAppConfig_Serialize>

const clashConfig = {
  enable_tun_mode: false,
  mixed_port: { kind: 'fixed', start_port: 7890 },
  socks_port: { kind: 'fixed', start_port: 7891 },
} satisfies Partial<ClashConfig>

const degraded: MutationOutcome<null> = {
  status: 'committed_degraded',
  value: null,
  commits: [],
  notifications_pending: false,
  degradations: [],
}

async function setup<T>(
  useHook: () => T,
  onTestFinished: TestContext['onTestFinished'],
) {
  const settled: unknown[] = []
  const testRpc = createTestRpc({
    get_app_config: async () => appConfig,
    get_clash_config: async () => clashConfig,
    patch_app_config: async () => degraded,
    patch_clash_config: async () => degraded,
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
  const hook = await renderHook(useHook, {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
    testRpc.invoke.mockReset()
  })
  return { hook, settled, invoke: testRpc.invoke }
}

test('useSetting reads and patches a typed application field', async ({
  onTestFinished,
}) => {
  const { hook, settled, invoke } = await setup(
    () => useSetting('core'),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current.value).toBe('mihomo')

  await hook.act(async () => {
    await hook.result.current.upsert('clash-rs')
  })

  expect(invoke).toHaveBeenCalledWith('patch_app_config', {
    patch: { core: 'clash-rs' },
  })
  expect(settled).toEqual([degraded])
})

test('useSettings passes the mutation outcome through', async ({
  onTestFinished,
}) => {
  const { hook, invoke } = await setup(() => useSettings(), onTestFinished)
  await expect
    .poll(() => hook.result.current.query.data?.theme_mode)
    .toBe('dark')

  let outcome: unknown
  await hook.act(async () => {
    outcome = await hook.result.current.upsert.mutateAsync({
      theme_mode: 'light',
    })
  })

  expect(outcome).toEqual(degraded)
  expect(invoke).toHaveBeenCalledWith('patch_app_config', {
    patch: { theme_mode: 'light' },
  })
})

test('a refused settings mutation preserves RpcFailure and never reports success', async ({
  onTestFinished,
}) => {
  const { hook, settled, invoke } = await setup(
    () => useSettings(),
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.query.data?.theme_mode)
    .toBe('dark')
  const failure = new RpcFailure('patch_app_config', {
    kind: 'application_error',
    message: 'Refused',
    domain_error: {
      kind: { domain: 'config', error: { kind: 'shutting_down' } },
      message: 'Refused',
      detail: 'Owner is shutting down',
    },
  })
  invoke.mockRejectedValueOnce(failure)

  await hook.act(async () => {
    await expect(
      hook.result.current.upsert.mutateAsync({ theme_mode: 'light' }),
    ).rejects.toBe(failure)
  })

  expect(hook.result.current.upsert.error).toBe(failure)
  expect(hook.result.current.query.data?.theme_mode).toBe('dark')
  expect(settled).toEqual([])
})

test('useClashSetting sends only the changed sub-field', async ({
  onTestFinished,
}) => {
  const { hook, settled, invoke } = await setup(
    () => useClashSetting('mixed_port'),
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.value)
    .toEqual({ kind: 'fixed', start_port: 7890 })

  await hook.act(async () => {
    await hook.result.current.upsert({ start_port: 7899 })
  })

  expect(invoke).toHaveBeenCalledWith('patch_clash_config', {
    patch: { mixed_port: { start_port: 7899 } },
  })
  expect(settled).toEqual([degraded])
})

test('useClashSetting clears an optional field with an explicit null', async ({
  onTestFinished,
}) => {
  const { hook, settled, invoke } = await setup(
    () => useClashSetting('socks_port'),
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.value)
    .toEqual({ kind: 'fixed', start_port: 7891 })

  await hook.act(async () => {
    await hook.result.current.upsert(null)
  })

  expect(invoke).toHaveBeenCalledWith('patch_clash_config', {
    patch: { socks_port: null },
  })
  expect(settled).toEqual([degraded])
})

test('upsert takes null only for a field null clears', () => {
  type AppValue = Parameters<ReturnType<typeof useSetting<'core'>>['upsert']>[0]
  type ClashValue = Parameters<
    ReturnType<typeof useClashSetting<'enable_tun_mode'>>['upsert']
  >[0]
  type ClearableValue = Parameters<
    ReturnType<typeof useClashSetting<'socks_port'>>['upsert']
  >[0]

  // Any other field reads JSON null as "not set": the call would commit
  // nothing and still succeed.
  // @ts-expect-error null is not a value of an application field
  const app: AppValue = null
  // @ts-expect-error null is not a value of a required clash field
  const clash: ClashValue = null
  const cleared: ClearableValue = null

  expect([app, clash, cleared]).toEqual([null, null, null])
})

test('ordinary settings writes exclude dedicated fields', () => {
  type Patch = Parameters<
    ReturnType<typeof useSettings>['upsert']['mutateAsync']
  >[0]
  type ServiceValue = Parameters<
    ReturnType<typeof useSetting<'enable_service_mode'>>['upsert']
  >[0]
  type HotkeysValue = Parameters<
    ReturnType<typeof useSetting<'hotkeys'>>['upsert']
  >[0]

  // @ts-expect-error service mode has a dedicated command
  const servicePatch: Patch = { enable_service_mode: false }
  // @ts-expect-error hotkeys have a dedicated command
  const hotkeysPatch: Patch = { hotkeys: [] }
  // @ts-expect-error service-mode fields are read-only through useSetting
  const serviceValue: ServiceValue = false
  // @ts-expect-error hotkey fields are read-only through useSetting
  const hotkeysValue: HotkeysValue = []
  const ordinary: Patch = { theme_mode: 'dark' }

  expect([
    servicePatch,
    hotkeysPatch,
    serviceValue,
    hotkeysValue,
    ordinary,
  ]).toEqual([
    { enable_service_mode: false },
    { hotkeys: [] },
    false,
    [],
    { theme_mode: 'dark' },
  ])
})

test('dedicated settings remain readable', async ({ onTestFinished }) => {
  const { hook } = await setup(
    () => ({
      service: useSetting('enable_service_mode').value,
      hotkeys: useSetting('hotkeys').value,
    }),
    onTestFinished,
  )

  await expect.poll(() => hook.result.current.service).toBe(false)
  expect(hook.result.current.hotkeys).toEqual(appConfig.hotkeys)
})

test('useClashSettings passes the mutation outcome through', async ({
  onTestFinished,
}) => {
  const { hook, invoke } = await setup(() => useClashSettings(), onTestFinished)
  await expect
    .poll(() => hook.result.current.query.data?.enable_tun_mode)
    .toBe(false)

  let outcome: unknown
  await hook.act(async () => {
    outcome = await hook.result.current.upsert.mutateAsync({
      enable_tun_mode: true,
    })
  })

  expect(outcome).toEqual(degraded)
  expect(invoke).toHaveBeenCalledWith('patch_clash_config', {
    patch: { enable_tun_mode: true },
  })
})

test('a patch resolves only after the config is read back', async ({
  onTestFinished,
}) => {
  let saved: Partial<NyanpasuAppConfig_Serialize> = appConfig
  let reads = 0
  let releaseReadBack = () => {}
  const readBack = new Promise<void>((resolve) => {
    releaseReadBack = resolve
  })
  const testRpc = createTestRpc({
    get_app_config: async () => {
      // The read after the patch is held until the test lets it finish.
      if (reads++ > 0) await readBack
      return saved
    },
    patch_app_config: async (params) => {
      saved = { ...saved, ...(params?.patch as typeof saved) }
      return degraded
    },
  })
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  const hook = await renderHook(() => useSetting('theme_mode'), {
    wrapper: rpcWrapper(testRpc.rpc, client),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    testRpc.rpc.dispose()
  })
  await expect.poll(() => hook.result.current.value).toBe('dark')

  let resolved = false
  const patched = hook.result.current.upsert('light').then(() => {
    resolved = true
  })
  await expect.poll(() => reads).toBe(2)
  expect(resolved).toBe(false)

  releaseReadBack()
  await hook.act(() => patched)

  // A second edit builds on this value, so it must already be the saved one.
  expect(hook.result.current.value).toBe('light')
})
