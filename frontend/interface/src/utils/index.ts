import type { IpcError } from '../ipc/rpc-bindings'

export type Result<T, E> =
  { status: 'ok'; data: T } | { status: 'error'; error: E }

/**
 * Unwrap a Tauri/specta Result envelope.
 * Returns T on ok, throws the error payload on error, and fails closed if the
 * runtime shape is neither (wire drift must not collapse to `undefined`).
 */
export function unwrapResult<T, E>(res: Result<T, E>): T {
  switch (res.status) {
    case 'ok':
      return res.data
    case 'error':
      if (
        typeof res.error === 'object' &&
        res.error !== null &&
        'message' in res.error
      ) {
        // Preserve machine-readable metadata while giving UI error handlers
        // a useful Error message and string representation.
        throw Object.assign(new Error(String(res.error.message)), res.error)
      }
      throw res.error
    default: {
      const _exhaustive: never = res
      throw new Error(
        `unexpected Result status: ${JSON.stringify(_exhaustive)}`,
      )
    }
  }
}

/** Whether a thrown value is the error payload of a failed command. */
export function isIpcError(value: unknown): value is IpcError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'kind' in value &&
    typeof (value as IpcError).message === 'string' &&
    typeof (value as IpcError).detail === 'string'
  )
}

export * from './get-system'
export * from './retry'
