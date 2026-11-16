import { detectRuntime, type Bridge, type Surface } from '@nyanpasu/platform'
import { getClientId } from './client-id'
import { RpcFailure, type RpcError } from './rpc-failure'
import { commands as tauriCommands } from './tauri-bindings'

export interface CommandTransportOptions {
  /** Called when the server rejects the caller's credentials (HTTP `401`). */
  onUnauthorized?: () => void
}

export interface RpcCommandTransport {
  invoke<T>(method: string, params?: Record<string, unknown>): Promise<T>
}

const rpcPath = '/bridge/rpc'

function isRpcError(value: unknown): value is RpcError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'kind' in value &&
    typeof value.kind === 'string' &&
    'message' in value &&
    typeof value.message === 'string'
  )
}

async function invokeHttpCommand<T>(
  method: string,
  params: Record<string, unknown>,
  options: CommandTransportOptions,
): Promise<T> {
  const response = await fetch(rpcPath, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-nyanpasu-client-id': getClientId(),
    },
    body: JSON.stringify({ method, params }),
  })

  if (response.status === 401) {
    const body: unknown = await response.json().catch(() => null)
    options.onUnauthorized?.()
    throw new RpcFailure(
      method,
      isRpcError(body)
        ? { ...body, kind: 'unauthorized' }
        : { kind: 'unauthorized', message: 'Authentication required' },
    )
  }

  let body: unknown
  try {
    body = await response.json()
  } catch {
    throw new Error(
      `RPC returned a non-JSON response (HTTP ${response.status})`,
    )
  }

  if (!response.ok) {
    if (isRpcError(body)) throw new RpcFailure(method, body)
    throw new Error(`RPC failed with HTTP ${response.status}`)
  }

  return body as T
}

/** Builds a command adapter with no process-wide state. */
export function createCommandTransport(
  bridge: Bridge = detectRuntime().bridge,
  surface: Surface = detectRuntime().platform.surface,
  options: CommandTransportOptions = {},
): RpcCommandTransport {
  return {
    async invoke<T>(method: string, params: Record<string, unknown> = {}) {
      if (bridge === 'none' && surface === 'browser') {
        return invokeHttpCommand<T>(method, params, options)
      }
      if (bridge !== 'tauri')
        throw new Error('No RPC transport is available for this runtime')
      const result = await tauriCommands.callRpc(method, params)
      if (result.status === 'error') throw new RpcFailure(method, result.error)
      return result.data as T
    },
  }
}
