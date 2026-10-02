import { expect, test, vi, type TestContext } from 'vitest'
import { renderHook } from 'vitest-browser-react'
import { createRpcClient, type RpcEventTransport } from '@nyanpasu/rpc'
import {
  QueryClient,
  QueryClientProvider,
  useQueryClient,
} from '@tanstack/react-query'
import {
  RpcProvider,
  useQueryBindings,
  useRpc,
} from '../src/provider/rpc-provider'

function makeRpc(owner: string) {
  const invokeMock = vi.fn(
    async (method: string, _params?: Record<string, unknown>) => ({
      owner,
      method,
    }),
  )
  const invoke = async <T,>(method: string, params?: Record<string, unknown>) =>
    (await invokeMock(method, params)) as T
  const eventTransport: RpcEventTransport = {
    listen: vi.fn(async () => () => {}),
    once: vi.fn(async () => () => {}),
    emit: vi.fn(async () => {}),
    listenMutation: vi.fn(async () => () => {}),
    listenResync: vi.fn(() => () => {}),
    dispose: vi.fn(),
  }
  return {
    rpc: createRpcClient({ commands: { invoke }, events: eventTransport }),
    invoke: invokeMock,
  }
}

function wrapperFor(
  rpc: ReturnType<typeof createRpcClient>,
  client: QueryClient,
) {
  return ({ children }: { children: React.ReactNode }) => (
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </RpcProvider>
  )
}

async function setup(
  rpcA: ReturnType<typeof createRpcClient>,
  rpcB: ReturnType<typeof createRpcClient>,
  clientA: QueryClient,
  clientB: QueryClient,
  onTestFinished: TestContext['onTestFinished'],
) {
  const first = await renderHook(
    () => ({
      rpc: useRpc(),
      bindings: useQueryBindings(),
      queryClient: useQueryClient(),
    }),
    { wrapper: wrapperFor(rpcA, clientA) },
  )
  const second = await renderHook(
    () => ({
      rpc: useRpc(),
      bindings: useQueryBindings(),
      queryClient: useQueryClient(),
    }),
    { wrapper: wrapperFor(rpcB, clientB) },
  )
  onTestFinished(async () => {
    await first.unmount()
    await second.unmount()
    rpcA.dispose()
    rpcB.dispose()
    clientA.clear()
    clientB.clear()
  })
  return { first, second }
}

test('RPC bindings and query cache stay scoped to each provider instance', async ({
  onTestFinished,
}) => {
  const { rpc: rpcA, invoke: invokeA } = makeRpc('first')
  const { rpc: rpcB, invoke: invokeB } = makeRpc('second')
  const clientA = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const clientB = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const { first, second } = await setup(
    rpcA,
    rpcB,
    clientA,
    clientB,
    onTestFinished,
  )

  expect(first.result.current.rpc).toBe(rpcA)
  expect(second.result.current.rpc).toBe(rpcB)
  expect(first.result.current.queryClient).toBe(clientA)
  expect(second.result.current.queryClient).toBe(clientB)

  const firstOptions = first.result.current.bindings.queries.getProfiles()
  const secondOptions = second.result.current.bindings.queries.getProfiles()
  await Promise.all([
    clientA.fetchQuery(firstOptions),
    clientB.fetchQuery(secondOptions),
  ])

  expect(invokeA).toHaveBeenCalledWith('get_profiles', undefined)
  expect(invokeB).toHaveBeenCalledWith('get_profiles', undefined)
  expect(clientA.getQueryData(['getProfiles'])).toEqual({
    status: 'ok',
    data: { owner: 'first', method: 'get_profiles' },
  })
  expect(clientB.getQueryData(['getProfiles'])).toEqual({
    status: 'ok',
    data: { owner: 'second', method: 'get_profiles' },
  })
  expect(clientA.getQueryData(['getProfiles'])).not.toBe(
    clientB.getQueryData(['getProfiles']),
  )
})
