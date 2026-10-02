import { describe, expect, it, vi } from 'vitest'
import {
  createRpcClient,
  isIpcError,
  unwrapResult,
  type RpcEventTransport,
  type RpcTransport,
} from '../src'

function fakeTransport() {
  const invokeMock = vi.fn(
    async (method: string, params?: Record<string, unknown>) => ({
      method,
      params,
    }),
  )
  const invoke = async <T>(method: string, params?: Record<string, unknown>) =>
    (await invokeMock(method, params)) as T
  const unlisten = vi.fn()
  const dispose = vi.fn()
  const listenMutation = vi.fn(async () => unlisten)
  const events: RpcEventTransport = {
    listen: vi.fn(async () => unlisten),
    once: vi.fn(async () => unlisten),
    emit: vi.fn(async () => {}),
    listenMutation,
    listenResync: vi.fn(() => vi.fn()),
    dispose,
  }
  const transport: RpcTransport = { commands: { invoke }, events }
  return {
    transport,
    invoke: invokeMock,
    events,
    unlisten,
    listenMutation,
    dispose,
  }
}

describe('RPC client instances', () => {
  it('keeps command and event lifecycles independent between clients', async () => {
    const first = fakeTransport()
    const second = fakeTransport()
    const firstClient = createRpcClient(first.transport)
    const secondClient = createRpcClient(second.transport)

    await firstClient.getProfiles()
    await secondClient.inspectRuntimeNode('snap-2', 4)
    const firstStop = await firstClient.listenMutation(vi.fn())
    const secondStop = await secondClient.listenMutation(vi.fn())
    firstStop()

    expect(first.invoke).toHaveBeenCalledWith('get_profiles', undefined)
    expect(second.invoke).toHaveBeenCalledWith('inspect_runtime_node', {
      snapshotId: 'snap-2',
      nodeId: 4,
    })
    expect(first.listenMutation).toHaveBeenCalledOnce()
    expect(second.listenMutation).toHaveBeenCalledOnce()
    expect(first.unlisten).toHaveBeenCalledOnce()
    expect(second.unlisten).not.toHaveBeenCalled()

    firstClient.dispose()
    expect(first.dispose).toHaveBeenCalledOnce()
    expect(second.dispose).not.toHaveBeenCalled()
    secondStop()
    secondClient.dispose()
    expect(second.unlisten).toHaveBeenCalledOnce()
    expect(second.dispose).toHaveBeenCalledOnce()
  })

  it('preserves structured error payloads through result helpers', async () => {
    const clientTransport = fakeTransport()
    const domainError = {
      kind: 'profile_not_found',
      message: 'Profile missing',
      detail: 'uid=missing',
    }
    clientTransport.invoke.mockRejectedValueOnce(domainError)
    const client = createRpcClient(clientTransport.transport)

    await expect(client.getProfiles()).resolves.toEqual({
      status: 'error',
      error: domainError,
    })
    expect(isIpcError(domainError)).toBe(true)
    expect(() => unwrapResult({ status: 'error', error: domainError })).toThrow(
      expect.objectContaining({
        kind: domainError.kind,
        detail: domainError.detail,
      }),
    )
    expect(unwrapResult({ status: 'ok', data: 'ready' })).toBe('ready')
  })
})
