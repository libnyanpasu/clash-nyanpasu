import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import type {
  ClashConfig,
  MutationOutcome,
  NyanpasuAppConfig_Serialize,
} from '../src/ipc/bindings'
import {
  useClashSetting,
  useClashSettings,
  useSetting,
  useSettings,
} from '../src/ipc/use-settings'

const ipc = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
}))
vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@tauri-apps/api/core')>()),
  invoke: ipc.invoke,
}))

const appConfig = {
  theme_mode: 'dark',
  core: 'mihomo',
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
  ipc.invoke.mockImplementation(async (command) => {
    switch (command) {
      case 'get_app_config':
        return appConfig
      case 'get_clash_config':
        return clashConfig
      case 'patch_app_config':
      case 'patch_clash_config':
        return degraded
      default:
        throw new Error(`Unexpected command: ${command}`)
    }
  })
  const hook = await renderHook(useHook, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  })
  onTestFinished(async () => {
    await hook.unmount()
    client.clear()
    ipc.invoke.mockReset()
  })
  return { hook, settled }
}

test('useSetting reads and patches a typed application field', async ({
  onTestFinished,
}) => {
  const { hook, settled } = await setup(
    () => useSetting('core'),
    onTestFinished,
  )
  await expect.poll(() => hook.result.current.value).toBe('mihomo')

  await hook.act(async () => {
    await hook.result.current.upsert('clash-rs')
  })

  expect(ipc.invoke).toHaveBeenCalledWith('patch_app_config', {
    patch: { core: 'clash-rs' },
  })
  expect(settled).toEqual([degraded])
})

test('useSettings passes the mutation outcome through', async ({
  onTestFinished,
}) => {
  const { hook } = await setup(() => useSettings(), onTestFinished)
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
  expect(ipc.invoke).toHaveBeenCalledWith('patch_app_config', {
    patch: { theme_mode: 'light' },
  })
})

test('useClashSetting sends only the changed sub-field', async ({
  onTestFinished,
}) => {
  const { hook, settled } = await setup(
    () => useClashSetting('mixed_port'),
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.value)
    .toEqual({ kind: 'fixed', start_port: 7890 })

  await hook.act(async () => {
    await hook.result.current.upsert({ start_port: 7899 })
  })

  expect(ipc.invoke).toHaveBeenCalledWith('patch_clash_config', {
    patch: { mixed_port: { start_port: 7899 } },
  })
  expect(settled).toEqual([degraded])
})

test('useClashSetting clears an optional field with an explicit null', async ({
  onTestFinished,
}) => {
  const { hook, settled } = await setup(
    () => useClashSetting('socks_port'),
    onTestFinished,
  )
  await expect
    .poll(() => hook.result.current.value)
    .toEqual({ kind: 'fixed', start_port: 7891 })

  await hook.act(async () => {
    await hook.result.current.upsert(null)
  })

  expect(ipc.invoke).toHaveBeenCalledWith('patch_clash_config', {
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

test('useClashSettings passes the mutation outcome through', async ({
  onTestFinished,
}) => {
  const { hook } = await setup(() => useClashSettings(), onTestFinished)
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
  expect(ipc.invoke).toHaveBeenCalledWith('patch_clash_config', {
    patch: { enable_tun_mode: true },
  })
})
