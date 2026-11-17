import { useState, type PropsWithChildren } from 'react'
import { CapabilitiesProvider } from '@/components/auth/capabilities'
import { KvStorageProvider, RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient, type RpcClient } from '@nyanpasu/rpc'
import type { RpcDescribeResponse } from '@nyanpasu/rpc/types'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'

/** The desktop owner: every resource written. */
export const DEFAULT_TEST_CAPABILITIES: RpcDescribeResponse = {
  protocol: 2,
  server: { kind: 'desktop', version: '0.0.0-test' },
  transport: 'desktop',
  principal: { kind: 'desktop', window: 'main' },
  role: 'owner',
  permissions: {
    proxy: 'write',
    profile: 'write',
    settings: 'write',
    system: 'write',
    logs: 'write',
    traffic: 'write',
    remote: 'write',
  },
  commands: [],
}

export function TestQueryProvider({
  client,
  rpc: injectedRpc,
  capabilities = DEFAULT_TEST_CAPABILITIES,
  children,
}: PropsWithChildren<{
  client: QueryClient
  rpc?: RpcClient
  capabilities?: RpcDescribeResponse
}>) {
  const [rpc] = useState(() => injectedRpc ?? createRpcClient())

  return (
    <CapabilitiesProvider snapshot={capabilities}>
      <RpcProvider rpc={rpc}>
        <QueryClientProvider client={client}>
          <KvStorageProvider>{children}</KvStorageProvider>
        </QueryClientProvider>
      </RpcProvider>
    </CapabilitiesProvider>
  )
}
