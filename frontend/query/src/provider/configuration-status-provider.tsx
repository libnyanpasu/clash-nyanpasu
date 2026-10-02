import {
  createContext,
  useContext,
  useEffect,
  type PropsWithChildren,
} from 'react'
import { unwrapResult } from '@nyanpasu/rpc'
import type { ConfigurationStatus, EffectKind } from '@nyanpasu/rpc/types'
import {
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { acceptConfigurationStatus } from '../ipc/configuration-status'
import { invokeMutation, MutationUnconfirmedError } from '../ipc/query-options'
import { useQueryApi } from './rpc-provider'

export const CONFIGURATION_STATUS_QUERY_KEY = [
  'getConfigurationStatus',
] as const

type ConfigurationStatusContextValue = {
  status: ConfigurationStatus | undefined
  isLoading: boolean
  isError: boolean
  error: unknown
  refetch: () => Promise<unknown>
  retryRuntime: () => Promise<void>
  retryEffect: (kind: EffectKind) => Promise<void>
  isRetrying: boolean
  retryError: unknown
  isLatestOperationUnconfirmed: boolean
}

const CONFIGURATION_MUTATIONS = new Set<unknown>([
  'patchAppConfig',
  'patchClashConfig',
  'patchRuntimeOverrides',
  'changeClashCore',
  'setHotkeys',
  'createProfile',
  'updateProfile',
  'patchProfileMetadata',
  'patchRemoteProfileOptions',
  'replaceProfileDefinition',
  'activateProfile',
  'setProfileValidFields',
  'reorderProfilesByList',
  'deleteProfile',
  'saveProfileFile',
])

const ConfigurationStatusContext =
  createContext<ConfigurationStatusContextValue | null>(null)

/** Shares configuration health polling and events for a mounted dashboard. */
export function ConfigurationStatusProvider({
  enabled = true,
  children,
}: PropsWithChildren<{ enabled?: boolean }>) {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: CONFIGURATION_STATUS_QUERY_KEY,
    enabled,
    queryFn: async () =>
      acceptConfigurationStatus(
        queryClient.getQueryData<ConfigurationStatus>(
          CONFIGURATION_STATUS_QUERY_KEY,
        ),
        await api.getConfigurationStatus(),
      ),
    refetchInterval: enabled ? 10_000 : false,
  })

  useEffect(() => {
    if (!enabled) return

    let disposed = false
    let unlisten: (() => void) | undefined
    api.events.configurationStatusChanged
      .listen((event) =>
        queryClient.setQueryData<ConfigurationStatus>(
          CONFIGURATION_STATUS_QUERY_KEY,
          (previous) => acceptConfigurationStatus(previous, event.payload),
        ),
      )
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch((error: unknown) => {
        if (!disposed)
          console.error('failed to subscribe to configuration status:', error)
      })
    const stopResync = api.listenResync(() => {
      queryClient
        .invalidateQueries({ queryKey: CONFIGURATION_STATUS_QUERY_KEY })
        .catch((error: unknown) =>
          console.error('failed to resync configuration status:', error),
        )
    })

    return () => {
      disposed = true
      stopResync()
      unlisten?.()
    }
  }, [api, enabled, queryClient])

  const retry = useMutation({
    mutationFn: async (kind: EffectKind | undefined) =>
      unwrapResult(
        await invokeMutation(
          {
            mutationFn: () =>
              kind
                ? api.retryConfigurationEffect(kind)
                : api.retryConfigurationRuntime(),
          },
          undefined,
        ),
      ),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: CONFIGURATION_STATUS_QUERY_KEY,
      }),
    onError: async (error) => {
      if (error instanceof MutationUnconfirmedError) await query.refetch()
    },
  })
  const configurationMutations = useMutationState({
    filters: {
      predicate: (mutation) =>
        CONFIGURATION_MUTATIONS.has(mutation.options.mutationKey?.[0]),
    },
    select: (mutation) => mutation.state,
  })
  let latestMutation: (typeof configurationMutations)[number] | undefined
  for (const mutation of configurationMutations) {
    if (!latestMutation || mutation.submittedAt > latestMutation.submittedAt)
      latestMutation = mutation
  }

  const value: ConfigurationStatusContextValue = {
    status: query.data,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    refetch: async () => {
      const result = await query.refetch()
      if (!result.isError) retry.reset()
      return result
    },
    retryRuntime: async () => {
      await retry.mutateAsync(undefined)
    },
    retryEffect: async (kind) => {
      await retry.mutateAsync(kind)
    },
    isRetrying: retry.isPending,
    retryError: retry.error,
    isLatestOperationUnconfirmed:
      latestMutation?.error instanceof MutationUnconfirmedError,
  }

  return (
    <ConfigurationStatusContext.Provider value={value}>
      {children}
    </ConfigurationStatusContext.Provider>
  )
}

export function useConfigurationStatus() {
  const value = useContext(ConfigurationStatusContext)
  if (!value) {
    throw new Error(
      'useConfigurationStatus must be used within a ConfigurationStatusProvider',
    )
  }
  return value
}
