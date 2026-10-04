import { useEffect, type PropsWithChildren } from 'react'
import type { RpcClient } from '@nyanpasu/rpc'
import type { Degradation } from '@nyanpasu/rpc/types'
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query'
import {
  ClashConnectionDetailsFreezeBoundary,
  ClashConnectionDetailsProvider,
  useClashConnectionDetails,
} from './clash-connection-details-provider'
import {
  ClashWSFreezeBoundary,
  ClashWSProvider,
  useClashWSHistory,
  useClashWSStatus,
} from './clash-ws-provider'
import {
  ConfigurationStatusProvider,
  useConfigurationStatus,
} from './configuration-status-provider'
import { MutationProvider } from './mutation-provider'
import { RpcProvider } from './rpc-provider'

export {
  RpcProvider,
  useQueryApi,
  useQueryBindings,
  useRpc,
} from './rpc-provider'
export { MutationProvider }
export { ConfigurationStatusProvider, useConfigurationStatus }

export type NyanpasuQueryProviderProps = PropsWithChildren<{
  rpc: RpcClient
  queryClient: QueryClient
  onDegraded?: (degradations: Degradation[]) => void
}>

export const NyanpasuQueryProvider = ({
  rpc,
  queryClient,
  onDegraded,
  children,
}: NyanpasuQueryProviderProps) => {
  useEffect(() => {
    if (!onDegraded) return
    return queryClient.getMutationCache().subscribe((event) => {
      if (event.type !== 'updated' || event.action.type !== 'success') return
      const data: unknown = event.mutation.state.data
      if (
        !data ||
        typeof data !== 'object' ||
        !('status' in data) ||
        data.status !== 'committed_degraded'
      )
        return
      if ('degradations' in data && Array.isArray(data.degradations)) {
        onDegraded(data.degradations as Degradation[])
      }
    })
  }, [onDegraded, queryClient])

  return (
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={queryClient}>
        <MutationProvider>
          <ClashWSProvider>
            <ConnectionDetailsBridge>{children}</ConnectionDetailsBridge>
          </ClashWSProvider>
        </MutationProvider>
      </QueryClientProvider>
    </RpcProvider>
  )
}

const ConnectionDetailsBridge = ({ children }: PropsWithChildren) => {
  const { state } = useClashWSStatus()
  return (
    <ClashConnectionDetailsProvider connectorState={state}>
      {children}
    </ClashConnectionDetailsProvider>
  )
}

export {
  ClashConnectionDetailsFreezeBoundary,
  ClashWSFreezeBoundary,
  useClashConnectionDetails,
  useClashWSHistory,
  useClashWSStatus,
}
