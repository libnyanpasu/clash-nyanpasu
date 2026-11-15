import { useEffect, type PropsWithChildren } from 'react'
import type { RpcClient } from '@nyanpasu/rpc'
import type { Degradation, PublicDegradation } from '@nyanpasu/rpc/types'
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
import {
  KvStorageProvider,
  type KvStoragePersistence,
} from './kv-storage-provider'
import { MutationProvider } from './mutation-provider'
import { RpcProvider } from './rpc-provider'

export {
  RpcProvider,
  useQueryApi,
  useQueryBindings,
  useRpc,
} from './rpc-provider'
export {
  clearKvStorageLocalCache,
  readKvStorageLocalCache,
} from './kv-storage-provider'
export type {
  KvStoragePersistence,
  KvStorageProviderProps,
} from './kv-storage-provider'
export { KvStorageProvider, MutationProvider }
export { ConfigurationStatusProvider, useConfigurationStatus }

export type AnyDegradation = Degradation | PublicDegradation

export type NyanpasuQueryProviderProps = PropsWithChildren<{
  rpc: RpcClient
  queryClient: QueryClient
  onDegraded?: (degradations: AnyDegradation[]) => void
  kvPersistence?: KvStoragePersistence
  clashWsEnabled?: boolean
}>

export const NyanpasuQueryProvider = ({
  rpc,
  queryClient,
  onDegraded,
  kvPersistence = 'backend',
  clashWsEnabled = true,
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
        onDegraded(data.degradations as AnyDegradation[])
      }
    })
  }, [onDegraded, queryClient])

  return (
    <RpcProvider rpc={rpc}>
      <QueryClientProvider client={queryClient}>
        <MutationProvider>
          <KvStorageProvider persistence={kvPersistence}>
            <ClashWSProvider enabled={clashWsEnabled}>
              <ConnectionDetailsBridge>{children}</ConnectionDetailsBridge>
            </ClashWSProvider>
          </KvStorageProvider>
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
