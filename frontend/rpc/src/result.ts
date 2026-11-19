import type { IpcError } from './rpc-bindings'

const KNOWN_IPC_DOMAINS = new Set<string>([
  'profiles',
  'runtime',
  'config',
  'storage',
  'system_dns',
  'system_proxy',
  'effects',
  'http_server',
  'local_socket',
  'clash_read',
  'core_log_read',
  'profile_sync_read',
])

const KNOWN_PUBLIC_FAILURE_DOMAINS = new Set<string>([
  ...KNOWN_IPC_DOMAINS,
  'core',
])

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Whether a thrown value is the error payload of a failed command. */
export function isIpcError(value: unknown): value is IpcError {
  if (!isPlainObject(value)) {
    return false
  }
  if (typeof value.message !== 'string') {
    return false
  }
  if (
    value.detail !== undefined &&
    value.detail !== null &&
    typeof value.detail !== 'string'
  ) {
    return false
  }
  if (!isPlainObject(value.kind) || typeof value.kind.domain !== 'string') {
    return false
  }
  if (value.kind.domain === 'unknown') {
    return value.kind.error === undefined
  }
  if (!isPlainObject(value.kind.error)) {
    return false
  }
  if (value.kind.domain === 'public') {
    return (
      typeof value.kind.error.domain === 'string' &&
      KNOWN_PUBLIC_FAILURE_DOMAINS.has(value.kind.error.domain) &&
      isPlainObject(value.kind.error.error)
    )
  }
  return KNOWN_IPC_DOMAINS.has(value.kind.domain)
}
