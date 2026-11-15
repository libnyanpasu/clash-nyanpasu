import {
  createContext,
  useContext,
  useMemo,
  type PropsWithChildren,
  type ReactNode,
} from 'react'
import type {
  DescribedCommand,
  Principal,
  RpcDescribeResponse,
  Scope,
} from '@nyanpasu/rpc/types'

export type RpcScope = Scope
export type RpcDescribeCommand = DescribedCommand
export type RpcDescribeSnapshot = RpcDescribeResponse
export type RpcPrincipal = Principal

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
    const { principal, scopes, transport, commands } = snapshot
    // `remote_admin` controls exist only in the desktop app.
    const isDesktop = principal.kind === 'desktop'
    const granted = new Set(scopes)
    const hasScope = (scope: Scope) =>
      granted.has(scope) && (scope !== 'remote_admin' || isDesktop)
    const byName = new Map(commands.map((command) => [command.name, command]))

    return {
      principal,
      scopes,
      transport,
      isDesktop,
      hasScope,
      /** Whether every scope in `required` is held. */
      hasScopes: (required: readonly Scope[]) => required.every(hasScope),
      canCall: (name: string) => {
        const command = byName.get(name)
        return !!command && command.allowed && hasScope(command.scope)
      },
      /** Whether mutation controls guarded by `scope` must be unavailable. */
      isReadOnly: (scope: Scope) => !hasScope(scope),
    }
  }, [snapshot])
}

export type Capabilities = ReturnType<typeof useCapabilities>

type RequireCapabilityProps = PropsWithChildren<
  { fallback?: ReactNode } & (
    | { scope: Scope; scopes?: never; command?: never }
    | { scopes: readonly Scope[]; scope?: never; command?: never }
    | { command: string; scope?: never; scopes?: never }
  )
>

/** Renders `children` only when the principal holds the scope or may call the command. */
export function RequireCapability({
  scope,
  scopes,
  command,
  fallback = null,
  children,
}: RequireCapabilityProps) {
  const capabilities = useCapabilities()
  const allowed =
    scope !== undefined
      ? capabilities.hasScope(scope)
      : scopes !== undefined
        ? capabilities.hasScopes(scopes)
        : capabilities.canCall(command)

  return <>{allowed ? children : fallback}</>
}
