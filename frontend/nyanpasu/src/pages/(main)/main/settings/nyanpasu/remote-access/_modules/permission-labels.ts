import type { HeldPermissions } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import type { Access, Grant, Resource, Role } from '@nyanpasu/rpc/types'

export const RESOURCE_LABELS = {
  meta: m.settings_remote_access_resource_meta,
  proxy: m.settings_remote_access_resource_proxy,
  profile: m.settings_remote_access_resource_profile,
  settings: m.settings_remote_access_resource_settings,
  system: m.settings_remote_access_resource_system,
  logs: m.settings_remote_access_resource_logs,
  traffic: m.settings_remote_access_resource_traffic,
  remote: m.settings_remote_access_resource_remote,
} satisfies Record<Resource, () => string>

export const ACCESS_LABELS = {
  read: m.settings_remote_access_access_read,
  write: m.settings_remote_access_access_write,
} satisfies Record<Access, () => string>

export const ROLE_LABELS = {
  anonymous: m.settings_remote_access_role_anonymous,
  viewer: m.settings_remote_access_role_viewer,
  operator: m.settings_remote_access_role_operator,
  admin: m.settings_remote_access_role_admin,
  owner: m.settings_remote_access_role_owner,
} satisfies Record<Role, () => string>

/** A held permission map as `Resource: level` pairs, in resource order. */
export function permissionsLabel(held: HeldPermissions): string {
  const pairs = (Object.keys(RESOURCE_LABELS) as Resource[]).flatMap(
    (resource) => {
      const access = held[resource]
      return access
        ? [`${RESOURCE_LABELS[resource]()}: ${ACCESS_LABELS[access]()}`]
        : []
    },
  )
  return pairs.length > 0
    ? pairs.join(' · ')
    : m.settings_remote_access_access_none()
}

/** What a token or session was granted, for its list entry. */
export function grantLabel(grant: Grant): string {
  return 'role' in grant && grant.role
    ? ROLE_LABELS[grant.role]()
    : permissionsLabel(grant.permissions ?? {})
}
