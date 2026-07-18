type Result<T, E> = { status: 'ok'; data: T } | { status: 'error'; error: E }

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
      throw res.error
    default: {
      const _exhaustive: never = res
      throw new Error(
        `unexpected Result status: ${JSON.stringify(_exhaustive)}`,
      )
    }
  }
}

export * from './get-system'
export * from './retry'
