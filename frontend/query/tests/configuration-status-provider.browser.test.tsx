import { afterEach, expect, test, vi, type TestContext } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  createRpcClient,
  type RpcEventCallback,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import type { ConfigurationStatus } from '@nyanpasu/rpc/types'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  CONFIGURATION_STATUS_QUERY_KEY,
  ConfigurationStatusProvider,
  useConfigurationStatus,
} from '../src/provider/configuration-status-provider'
import { RpcProvider } from '../src/provider/rpc-provider'

const status = (event_seq: number): ConfigurationStatus => ({
  event_seq,
  maintenance: null,
  runtime: {
    health: 'healthy',
    operation_id: null,
    attempts: 0,
    automatic_remaining: 0,
    message: null,
  },
  source_versions: { application: 1, clash: 1, session: 1, profiles: 1 },
  effects: [],
  sources: [],
  recent_operations: [],
  active: null,
})

type EventHandler = RpcEventCallback<ConfigurationStatus>

function mockRpc(initial: ConfigurationStatus) {
  let handler: EventHandler | undefined
  let resyncHandler: (() => void) | undefined
  let sequence = initial.event_seq
  const invoke = vi.fn(
    async (method: string, _params?: Record<string, unknown>) => {
      if (method === 'get_configuration_status') return status(sequence)
      if (method === 'retry_configuration_runtime')
        return { status: 'ok', data: null }
      if (method === 'retry_configuration_effect')
        return { status: 'ok', data: null }
      throw new Error(`Unexpected RPC method: ${method}`)
    },
  )
  const commands = {
    invoke: async <T,>(method: string, params?: Record<string, unknown>) =>
      (await invoke(method, params)) as T,
  }
  const events: RpcEventTransport = {
    listen: vi.fn(async <T,>(name: string, callback: RpcEventCallback<T>) => {
      if (name === 'configuration-status-changed') {
        handler = callback as RpcEventCallback<ConfigurationStatus>
      }
      return () => {
        handler = undefined
      }
    }),
    once: vi.fn(async () => () => {}),
    emit: vi.fn(async () => {}),
    listenMutation: vi.fn(async () => () => {}),
    listenResync: vi.fn((callback: () => void) => {
      resyncHandler = callback
      return () => {
        resyncHandler = undefined
      }
    }),
    dispose: vi.fn(),
  }
  return {
    rpc: createRpcClient({ commands, events }),
    invoke,
    emit(next: ConfigurationStatus) {
      sequence = next.event_seq
      handler?.({
        event: 'configuration-status-changed',
        id: next.event_seq,
        payload: next,
      })
    },
    resync() {
      resyncHandler?.()
    },
    listenerCount: () => (handler ? 1 : 0),
    eventListen: events.listen,
  }
}

function Consumer({
  onValue,
}: {
  onValue: (value: ReturnType<typeof useConfigurationStatus>) => void
}) {
  const value = useConfigurationStatus()
  onValue(value)
  return null
}

async function setup(
  onTestFinished: TestContext['onTestFinished'],
  enabled = true,
) {
  const mock = mockRpc(status(1))
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  const values: ReturnType<typeof useConfigurationStatus>[] = []
  const screen = await render(
    <RpcProvider rpc={mock.rpc}>
      <QueryClientProvider client={queryClient}>
        <ConfigurationStatusProvider enabled={enabled}>
          <Consumer onValue={(value) => values.push(value)} />
          <Consumer onValue={(value) => values.push(value)} />
        </ConfigurationStatusProvider>
      </QueryClientProvider>
    </RpcProvider>,
  )
  onTestFinished(async () => {
    await screen.unmount()
    mock.rpc.dispose()
    queryClient.clear()
  })
  return { ...mock, queryClient, screen, values }
}

afterEach(() => vi.restoreAllMocks())

test('shares one status listener and accepts events without letting older data win', async ({
  onTestFinished,
}) => {
  const {
    eventListen,
    emit,
    invoke,
    listenerCount,
    queryClient,
    resync,
    values,
  } = await setup(onTestFinished)

  await expect.poll(() => invoke).toHaveBeenCalledTimes(1)
  await expect.poll(() => listenerCount()).toBe(1)
  expect(eventListen).toHaveBeenCalledTimes(1)

  emit(status(3))
  await expect
    .poll(
      () =>
        queryClient.getQueryData<ConfigurationStatus>(
          CONFIGURATION_STATUS_QUERY_KEY,
        )?.event_seq,
    )
    .toBe(3)
  await expect
    .poll(() => values.filter((value) => value.status?.event_seq === 3).length)
    .toBe(2)

  emit(status(2))
  await expect
    .poll(
      () =>
        queryClient.getQueryData<ConfigurationStatus>(
          CONFIGURATION_STATUS_QUERY_KEY,
        )?.event_seq,
    )
    .toBe(3)

  resync()
  await expect.poll(() => invoke).toHaveBeenCalledTimes(2)
})

test('does not query or subscribe while disabled', async ({
  onTestFinished,
}) => {
  const disabled = await setup(onTestFinished, false)
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(disabled.invoke).not.toHaveBeenCalled()
  expect(disabled.eventListen).not.toHaveBeenCalled()
  expect(disabled.listenerCount()).toBe(0)
})
