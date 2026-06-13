type Result<T, E> = { status: 'ok'; data: T } | { status: 'error'; error: E }

export function unwrapResult<T, E>(res: Result<T, E>) {
  if (res.status === 'error') {
    throw res.error
  }
  return res.status === 'ok' ? res.data : undefined
}

export * from './get-system'
export * from './retry'
