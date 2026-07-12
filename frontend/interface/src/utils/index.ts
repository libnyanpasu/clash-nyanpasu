type Result<T, E> = { status: 'ok'; data: T } | { status: 'error'; error: E }

export function unwrapResult<T, E>(res: Result<T, E>) {
  if (res.status === 'error') {
    throw res.error
  }
  return res.status === 'ok' ? res.data : undefined
}

/**
 * Extract a degraded rebuild error from a mutation's resolved payload.
 * Handles both wire shapes from PR-4: a bare `RebuildOutcome`
 * (`{status:'degraded',error}`) and a `CommitOutcome<T>` (`{value, rebuild}`),
 * plus locally normalized `{ uid, rebuild }` shapes.
 */
export const extractDegradedRebuild = (data: unknown): string | undefined => {
  if (!data || typeof data !== 'object') {
    return undefined
  }
  const outcome =
    'rebuild' in data ? (data as { rebuild?: unknown }).rebuild : data
  if (!outcome || typeof outcome !== 'object') {
    return undefined
  }
  const candidate = outcome as { status?: unknown; error?: unknown }
  return candidate.status === 'degraded' && typeof candidate.error === 'string'
    ? candidate.error
    : undefined
}

export * from './get-system'
export * from './retry'
