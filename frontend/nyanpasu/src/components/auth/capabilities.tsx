import {
  createContext,
  useContext,
  useMemo,
  type PropsWithChildren,
  type ReactNode,
} from 'react'

export type RpcScope =
  | 'read'
  | 'sensitive_read'
  | 'proxy'
  | 'profile'
  | 'settings'
  | 'system'
  | 'remote_admin'

export interface RpcDescribeCommand {
  name: string
  kind: 'query' | 'mutation'
  scope: RpcScope
  /** Whether this transport exposes the command and the principal's scope covers it. */
  allowed: boolean
}

/** The `rpc_describe` result: what the resolved principal may do. */
export interface RpcDescribeSnapshot {
  protocol: number
  server: { kind: string; version: string }
  transport: string
  principal: { kind: string; id: string; name?: string | null }
  scopes: RpcScope[]
  commands: RpcDescribeCommand[]
}

export const CapabilitiesContext = createContext<RpcDescribeSnapshot | null>(
  null,
)

export function CapabilitiesProvider({
  snapshot,
  children,
}: PropsWithChildren<{ snapshot: RpcDescribeSnapshot }>) {
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
    const { principal, scopes, transport, commands } = snapshot
    // `remote_admin` controls exist only in the desktop app.
    const isDesktop = principal.kind === 'desktop'
    const granted = new Set(scopes)
    const hasScope = (scope: RpcScope) =>
      granted.has(scope) && (scope !== 'remote_admin' || isDesktop)
    const byName = new Map(commands.map((command) => [command.name, command]))

    return {
      principal,
      scopes,
      transport,
      isDesktop,
      hasScope,
      /** Whether every scope in `required` is held. */
      hasScopes: (required: readonly RpcScope[]) => required.every(hasScope),
      canCall: (name: string) => {
        const command = byName.get(name)
        return !!command && command.allowed && hasScope(command.scope)
      },
      /** Whether mutation controls guarded by `scope` must be unavailable. */
      isReadOnly: (scope: RpcScope) => !hasScope(scope),
    }
  }, [snapshot])
}

export type Capabilities = ReturnType<typeof useCapabilities>

type RequireCapabilityProps = PropsWithChildren<
  { fallback?: ReactNode } & (
    { scope: RpcScope; command?: never } | { command: string; scope?: never }
  )
>

/** Renders `children` only when the principal holds the scope or may call the command. */
export function RequireCapability({
  scope,
  command,
  fallback = null,
  children,
}: RequireCapabilityProps) {
  const capabilities = useCapabilities()
  const allowed =
    scope !== undefined
      ? capabilities.hasScope(scope)
      : capabilities.canCall(command)

  return <>{allowed ? children : fallback}</>
}
