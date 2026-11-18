import { useEffect, useState, type ReactNode } from 'react'
import { RpcFailure } from '@nyanpasu/rpc'
import type { QueryClient } from '@tanstack/react-query'
import { CapabilitiesProvider, type RpcDescribeSnapshot } from './capabilities'
import {
  PRINCIPAL_MARKER_KEY,
  reconcilePrincipalMarker,
} from './principal-marker'

const LOGIN_PATH = '/login'

export interface AuthBootstrapGateProps {
  /** Resolves this page runtime's `rpc_describe` snapshot. */
  describe: () => Promise<RpcDescribeSnapshot>
  /** The page's query client, cleared when the principal changed. */
  queryClient: QueryClient
  /** Leaves for the login page; defaults to a document navigation. */
  navigateToLogin?: () => void
  /** Reloads the page to bind a new principal; defaults to `location.reload`. */
  reload?: () => void
  /** Shown while `describe` is pending. */
  pending?: ReactNode
  /** Shown when `describe` fails for a reason other than missing credentials. */
  renderError?: (error: unknown, retry: () => void) => ReactNode
  children: ReactNode
}

type GateState =
  | { status: 'pending' }
  | { status: 'ready'; snapshot: RpcDescribeSnapshot; marker: string }
  | { status: 'failed'; error: unknown }
  | { status: 'unauthorized' }

function isUnauthorized(error: unknown) {
  return error instanceof RpcFailure && error.envelope.kind === 'unauthorized'
}

/**
 * Binds the page to one principal before any provider mounts: unauthenticated
 * callers go to the login page, a principal or scope change clears the state
 * the previous one persisted, and only then do `children` render.
 */
export function AuthBootstrapGate({
  describe,
  queryClient,
  navigateToLogin = () => window.location.replace(LOGIN_PATH),
  reload = () => window.location.reload(),
  pending = null,
  renderError,
  children,
}: AuthBootstrapGateProps) {
  const [state, setState] = useState<GateState>({ status: 'pending' })
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    let cancelled = false
    setState({ status: 'pending' })

    describe().then(
      (snapshot) => {
        if (cancelled) return
        // Clear before the state change that renders `children`.
        const marker = reconcilePrincipalMarker(snapshot, queryClient)
        setState({ status: 'ready', snapshot, marker })
      },
      (error: unknown) => {
        if (cancelled) return
        if (isUnauthorized(error)) setState({ status: 'unauthorized' })
        else setState({ status: 'failed', error })
      },
    )

    return () => {
      cancelled = true
    }
    // The callbacks are the caller's wiring, not reasons to describe again.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [attempt])

  const unauthorized = state.status === 'unauthorized'
  useEffect(() => {
    if (unauthorized) navigateToLogin()
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [unauthorized])

  // Another tab signing in as someone else must not leave this one on the old
  // principal: a page runtime is bound to one principal for its lifetime.
  const marker = state.status === 'ready' ? state.marker : null
  useEffect(() => {
    if (marker === null) return
    const onStorage = (event: StorageEvent) => {
      if (event.key === PRINCIPAL_MARKER_KEY && event.newValue !== marker) {
        reload()
      }
    }
    window.addEventListener('storage', onStorage)
    return () => window.removeEventListener('storage', onStorage)
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [marker])

  switch (state.status) {
    case 'ready':
      return (
        <CapabilitiesProvider snapshot={state.snapshot}>
          {children}
        </CapabilitiesProvider>
      )
    case 'failed':
      return renderError
        ? renderError(state.error, () => setAttempt((count) => count + 1))
        : null
    default:
      return pending
  }
}
