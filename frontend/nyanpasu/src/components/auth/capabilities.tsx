import {
  createContext,
  useContext,
  useMemo,
  type PropsWithChildren,
  type ReactNode,
} from 'react'
import type {
  Access,
  DescribedCommand,
  Principal,
  Resource,
  RpcDescribeResponse,
} from '@nyanpasu/rpc/types'

export type RpcDescribeCommand = DescribedCommand
export type RpcDescribeSnapshot = RpcDescribeResponse
export type RpcPrincipal = Principal

/** A permission written as the backend displays it, such as `proxy:write`. */
export type PermissionName = `${Resource}:${Access}`

/** The levels a caller holds, by resource; a resource that is not listed is not held. */
export type HeldPermissions = RpcDescribeResponse['permissions']

/**
 * Whether `held` satisfies `required`: `write` includes `read`, and every
 * resolved caller holds `meta:read`.
 */
export function permits(held: HeldPermissions, required: PermissionName) {
  const [resource, access] = required.split(':') as [Resource, Access]
  if (resource === 'meta' && access === 'read') {
    return true
  }
  const level = held[resource]
  return level === 'write' || level === access
}

export const CapabilitiesContext = createContext<RpcDescribeResponse | null>(
  null,
)

export function CapabilitiesProvider({
  snapshot,
  children,
}: PropsWithChildren<{ snapshot: RpcDescribeResponse }>) {
  return (
    <CapabilitiesContext.Provider value={snapshot}>
      {children}
    </CapabilitiesContext.Provider>
  )
}

/** The immutable capability snapshot of this page runtime. */
export function useCapabilities() {
  const snapshot = useContext(CapabilitiesContext)
  if (!snapshot) {
    throw new Error('useCapabilities must be used within an AuthBootstrapGate')
  }

  return useMemo(() => {
    const { principal, role, permissions, transport, commands } = snapshot
    // `remote` controls exist only in the desktop app.
    const isDesktop = principal.kind === 'desktop'
    const hasPermission = (permission: PermissionName) =>
      permits(permissions, permission) &&
      (!permission.startsWith('remote:') || isDesktop)
    const byName = new Map(commands.map((command) => [command.name, command]))

    return {
      principal,
      role,
      permissions,
      transport,
      commands,
      isDesktop,
      hasPermission,
      /** Whether every permission in `required` is held. */
      hasPermissions: (required: readonly PermissionName[]) =>
        required.every(hasPermission),
      canCall: (name: string) => {
        const command = byName.get(name)
        return (
          !!command &&
          command.allowed &&
          hasPermission(
            `${command.permission.resource}:${command.permission.access}`,
          )
        )
      },
      /** Whether mutation controls guarded by `permission` must be unavailable. */
      isReadOnly: (permission: PermissionName) => !hasPermission(permission),
    }
  }, [snapshot])
}

export type Capabilities = ReturnType<typeof useCapabilities>

type RequireCapabilityProps = PropsWithChildren<
  { fallback?: ReactNode } & (
    | { permission: PermissionName; permissions?: never; command?: never }
    | {
        permissions: readonly PermissionName[]
        permission?: never
        command?: never
      }
    | { command: string; permission?: never; permissions?: never }
  )
>

/** Renders `children` only when the caller holds the permissions or may call the command. */
export function RequireCapability({
  permission,
  permissions,
  command,
  fallback = null,
  children,
}: RequireCapabilityProps) {
  const capabilities = useCapabilities()
  const allowed =
    permission !== undefined
      ? capabilities.hasPermission(permission)
      : permissions !== undefined
        ? capabilities.hasPermissions(permissions)
        : capabilities.canCall(command)

  return <>{allowed ? children : fallback}</>
}
