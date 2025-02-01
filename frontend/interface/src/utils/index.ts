import type { Result } from '../ipc/bindings'

export function unwrapResult<T, E>(res: Result<T, E>) {
  if (res.status === 'error') {
    throw res.error
  }
  return res.status === 'ok' ? res.data : undefined
}
