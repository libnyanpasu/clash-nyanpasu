/* oxlint-disable no-throw-literal -- tauri-specta wraps string rejections in its Result return */
import { isTauri } from '@tauri-apps/api/core'
import { commands as tauriCommands } from './tauri-bindings'

export interface RpcCommandTransport {
  invoke<T>(method: string, params?: Record<string, unknown>): Promise<T>
}

export interface RpcError {
  kind: string
  message: string
  code?: string | null
  retryable?: boolean | null
  operation_id?: string | null
  domain_error?: unknown
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
  params: Record<string, unknown> = {},
): Promise<T> {
  let response: Response
  try {
    response = await fetch(rpcPath, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ method, params }),
    })
  } catch (error) {
    throw new Error(
      error instanceof Error ? error.message : 'RPC request failed',
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
    if (isRpcError(body)) throw body.domain_error ?? body
    throw new Error(`RPC failed with HTTP ${response.status}`)
  }

  return body as T
}

/** Builds a command adapter with no process-wide state. */
export function createCommandTransport(): RpcCommandTransport {
  return {
    async invoke<T>(method: string, params: Record<string, unknown> = {}) {
      if (!isTauri()) {
        return invokeHttpCommand<T>(method, params)
      }
      const result = await tauriCommands.callRpc(method, params)
      if (result.status === 'error')
        throw result.error.domain_error ?? result.error
      return result.data as T
    },
  }
}
