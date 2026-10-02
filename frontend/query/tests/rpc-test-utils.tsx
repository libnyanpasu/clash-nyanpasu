import type { PropsWithChildren } from 'react'
import { vi } from 'vitest'
import {
  createRpcClient,
  type RpcClient,
  type RpcEventTransport,
} from '@nyanpasu/rpc'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RpcProvider } from '../src/provider/rpc-provider'

export type RpcHandler = (
  params?: Record<string, unknown>,
) => unknown | Promise<unknown>

export function createTestRpc(handlers: Record<string, RpcHandler>) {
  const listeners = new Map<
    string,
    Set<(event: { event: string; id: number; payload: unknown }) => void>
  >()
  const resyncListeners = new Set<() => void>()
  let sequence = 0
  const invokeMock = vi.fn(
    async (method: string, params?: Record<string, unknown>) => {
      const handler = handlers[method]
      if (!handler) throw new Error(`Unexpected RPC command: ${method}`)
      return await handler(params)
    },
  )
  const invoke = async <T,>(method: string, params?: Record<string, unknown>) =>
    (await invokeMock(method, params)) as T
  const listen = <T,>(
    name: string,
    callback: (event: { event: string; id: number; payload: T }) => void,
  ) => {
    let callbacks = listeners.get(name)
    if (!callbacks) listeners.set(name, (callbacks = new Set()))
    callbacks.add(
      callback as (event: {
        event: string
        id: number
        payload: unknown
      }) => void,
    )
    return Promise.resolve(() =>
      callbacks?.delete(
        callback as (event: {
          event: string
          id: number
          payload: unknown
        }) => void,
      ),
    )
  }
  const events: RpcEventTransport = {
    listen,
    once: <T,>(
      name: string,
      callback: (event: { event: string; id: number; payload: T }) => void,
    ) => {
      let stop: (() => void) | undefined
      return listen<T>(name, (event) => {
        stop?.()
        callback(event)
      }).then((unlisten) => {
        stop = unlisten
        return unlisten
      })
    },
    emit: async (name, payload) => {
      for (const callback of [...(listeners.get(name) ?? [])]) {
        callback({ event: name, id: sequence++, payload })
      }
    },
    listenMutation: (callback) =>
      listen('nyanpasu://mutation', ({ payload }) => callback(payload)),
    listenResync: (callback) => {
      resyncListeners.add(callback)
      return () => resyncListeners.delete(callback)
    },
    dispose: () => {
      listeners.clear()
      resyncListeners.clear()
    },
  }
  const rpc = createRpcClient({ commands: { invoke }, events })
  return {
    rpc,
    invoke: invokeMock,
    emitResync: () => [...resyncListeners].forEach((callback) => callback()),
  }
}

export function rpcWrapper(rpc: RpcClient, client: QueryClient) {
  return ({ children }: PropsWithChildren) => (
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </RpcProvider>
  )
}
