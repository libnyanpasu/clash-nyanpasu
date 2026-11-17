import type { PropsWithChildren } from 'react'
import {
  CapabilitiesProvider,
  type HeldPermissions,
} from '@/components/auth/capabilities'
import type { Principal, Role, RpcDescribeResponse } from '@nyanpasu/rpc/types'
import { DEFAULT_TEST_CAPABILITIES } from './query-provider'

/** The desktop principal, holding every permission: the default caller of a page under test. */
export function AllPermissions({ children }: PropsWithChildren) {
  return (
    <CapabilitiesProvider snapshot={DEFAULT_TEST_CAPABILITIES}>
      {children}
    </CapabilitiesProvider>
  )
}

/**
 * A wrapper for a caller holding exactly `permissions`, a token unless another
 * principal is given.
 */
export function withPermissions(
  permissions: HeldPermissions,
  principal: Principal = { kind: 'token', id: 'a1b2', name: 'laptop' },
  role: Role | null = null,
) {
  const snapshot: RpcDescribeResponse = {
    ...DEFAULT_TEST_CAPABILITIES,
    server: { kind: 'desktop', version: '0.0.0-test' },
    transport: 'http',
    principal,
    role,
    permissions,
  }

  return function PermittedCaller({ children }: PropsWithChildren) {
    return (
      <CapabilitiesProvider snapshot={snapshot}>
        {children}
      </CapabilitiesProvider>
    )
  }
}
