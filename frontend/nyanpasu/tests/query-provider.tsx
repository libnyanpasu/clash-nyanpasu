import { useState, type PropsWithChildren } from 'react'
import { KvStorageProvider, RpcProvider } from '@nyanpasu/query/provider'
import { createRpcClient, type RpcClient } from '@nyanpasu/rpc'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'

export function TestQueryProvider({
  client,
  rpc: injectedRpc,
  children,
}: PropsWithChildren<{ client: QueryClient; rpc?: RpcClient }>) {
  const [rpc] = useState(() => injectedRpc ?? createRpcClient())

  return (
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={client}>
        <KvStorageProvider>{children}</KvStorageProvider>
      </QueryClientProvider>
    </RpcProvider>
  )
}
