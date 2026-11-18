import type { PropsWithChildren } from 'react'
import { CapabilitiesProvider } from '@/components/auth/capabilities'
import type { Principal, RpcDescribeResponse, Scope } from '@nyanpasu/rpc/types'
import { DEFAULT_TEST_CAPABILITIES } from './query-provider'

/** The desktop principal, holding every scope: the default caller of a page under test. */
export function AllScopes({ children }: PropsWithChildren) {
  return (
    <CapabilitiesProvider snapshot={DEFAULT_TEST_CAPABILITIES}>
      {children}
    </CapabilitiesProvider>
  )
}

/** A wrapper for a caller holding exactly `scopes`, a token unless another principal is given. */
export function withScopes(
  scopes: Scope[],
  principal: Principal = { kind: 'token', id: 'a1b2', name: 'laptop' },
) {
  const snapshot: RpcDescribeResponse = {
    ...DEFAULT_TEST_CAPABILITIES,
    server: { kind: 'desktop', version: '0.0.0-test' },
    transport: 'http',
    principal,
    scopes,
  }

  return function ScopedCaller({ children }: PropsWithChildren) {
    return (
      <CapabilitiesProvider snapshot={snapshot}>
        {children}
      </CapabilitiesProvider>
    )
  }
}
