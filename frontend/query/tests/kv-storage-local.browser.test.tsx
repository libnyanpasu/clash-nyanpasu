import type { PropsWithChildren } from 'react'
import { beforeEach, expect, test } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { QueryClient } from '@tanstack/react-query'
import { useKvStorage } from '../src/hooks/use-kv-storage'
import {
  clearKvStorageLocalCache,
  KvStorageProvider,
} from '../src/provider/kv-storage-provider'
import { MutationProvider } from '../src/provider/mutation-provider'
import { createTestRpc, rpcWrapper } from './rpc-test-utils'

beforeEach(() => localStorage.clear())

const cacheKey = (key: string) => `nyanpasu-kv-:${btoa(key)}`

async function mountLocal(
  onTestFinished: (callback: () => Promise<void>) => void,
) {
  // Any backend command fails the test: there are no handlers.
  const testRpc = createTestRpc({})
  const client = new QueryClient()
  const Wrapper = rpcWrapper(testRpc.rpc, client)
  const Root = ({ children }: PropsWithChildren) => (
    <Wrapper>
      <MutationProvider>
        <KvStorageProvider persistence="local">{children}</KvStorageProvider>
      </MutationProvider>
    </Wrapper>
  )
  const hook = await renderHook(
    () => useKvStorage<string>('greeting', 'default'),
    { wrapper: Root },
  )
  onTestFinished(async () => {
    await hook.unmount()
    testRpc.rpc.dispose()
    client.clear()
  })
  return { hook, testRpc }
}

test('local persistence settles from the browser copy without the backend', async ({
  onTestFinished,
}) => {
  localStorage.setItem(cacheKey('greeting'), JSON.stringify('cached'))

  const { hook, testRpc } = await mountLocal(onTestFinished)

  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toBe('cached')
  expect(hook.result.current[2].readError).toBeNull()
  expect(testRpc.invoke).not.toHaveBeenCalled()
})

test('local persistence settles on the default when nothing is cached', async ({
  onTestFinished,
}) => {
  const { hook, testRpc } = await mountLocal(onTestFinished)

  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)
  expect(hook.result.current[0]).toBe('default')
  expect(testRpc.invoke).not.toHaveBeenCalled()
})

test('local persistence writes only to localStorage', async ({
  onTestFinished,
}) => {
  const { hook, testRpc } = await mountLocal(onTestFinished)
  await expect.poll(() => hook.result.current[2].isLoading).toBe(false)

  await expect(hook.result.current[1]('written')).resolves.toBe(true)

  await expect.poll(() => hook.result.current[0]).toBe('written')
  expect(localStorage.getItem(cacheKey('greeting'))).toBe(
    JSON.stringify('written'),
  )
  expect(testRpc.invoke).not.toHaveBeenCalled()
})

test('clearKvStorageLocalCache removes only the mirrored values', () => {
  localStorage.setItem(cacheKey('a'), '1')
  localStorage.setItem(cacheKey('b'), '2')
  localStorage.setItem('theme-palette-v1', '{}')

  clearKvStorageLocalCache()

  expect(localStorage.getItem(cacheKey('a'))).toBeNull()
  expect(localStorage.getItem(cacheKey('b'))).toBeNull()
  expect(localStorage.getItem('theme-palette-v1')).toBe('{}')
})
