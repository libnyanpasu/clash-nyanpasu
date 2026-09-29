/* oxlint-disable no-throw-literal -- tauri-specta wraps string rejections in its Result return */
import { commands as tauriCommands } from './bindings'

interface RpcError {
  kind: string
  message: string
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

export async function invokeHttpCommand<T>(
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
    if (isRpcError(body)) throw body.message
    throw new Error(`RPC failed with HTTP ${response.status}`)
  }

  return body as T
}

export async function invokeRpcCommand<T>(
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return invokeHttpCommand<T>(method, params)
  }
  const result = await tauriCommands.callRpc(method, params)
  if (result.status === 'error') throw result.error.message
  return result.data as T
}
