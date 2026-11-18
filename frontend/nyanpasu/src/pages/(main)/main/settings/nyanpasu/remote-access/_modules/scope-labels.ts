import { m } from '@/paraglide/messages'
import type { Scope } from '@nyanpasu/rpc/types'

/**
 * What an anonymous local caller can be given. Reading secrets, system control
 * and remote administration are never anonymous, and the backend rejects them.
 */
export const LOCAL_OPEN_SCOPES = [
  'read',
  'proxy',
  'profile',
  'settings',
] as const satisfies readonly Scope[]

/** What a token can hold; `remote_admin` belongs to the desktop app alone. */
export const TOKEN_SCOPES = [
  'read',
  'sensitive_read',
  'proxy',
  'profile',
  'settings',
  'system',
] as const satisfies readonly Scope[]

export function scopeLabel(scope: Scope): string {
  switch (scope) {
    case 'read':
      return m.settings_remote_access_scope_read()
    case 'sensitive_read':
      return m.settings_remote_access_scope_sensitive_read()
    case 'proxy':
      return m.settings_remote_access_scope_proxy()
    case 'profile':
      return m.settings_remote_access_scope_profile()
    case 'settings':
      return m.settings_remote_access_scope_settings()
    case 'system':
      return m.settings_remote_access_scope_system()
    case 'remote_admin':
      return scope
  }
}
