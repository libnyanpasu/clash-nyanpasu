import { kebabCase } from 'es-toolkit'
import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ClashCore_Deserialize,
  ClashCore_Serialize,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

export const ClashCores = {
  clash: 'Clash Premium',
  mihomo: 'Mihomo',
  'mihomo-alpha': 'Mihomo Alpha',
  'clash-rs': 'Clash Rust',
  'clash-rs-alpha': 'Clash Rust Alpha',
  meow: 'Meow',
} as Record<ClashCore_Serialize, string>

export type ClashCoresInfo = Record<ClashCore_Serialize, ClashCoresDetail>

export type ClashCoresDetail = {
  name: string
  currentVersion: string
  latestVersion?: string
}

export const useClashCores = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const coreQueryKey = api.queries.getCoreVersion('clash').queryKey.slice(0, 1)
  const fetchLatestCoreVersions = api.queries.fetchLatestCoreVersions()
  const updateCoreCommand = api.mutations.updateCore
  const changeClashCoreCommand = api.mutations.changeClashCore
  const restartSidecarCommand = api.mutations.restartSidecar

  const query = useQuery({
    queryKey: coreQueryKey,
    // Reading a version runs that core's binary; versions change only through
    // the update and switch mutations, which refetch them.
    staleTime: Infinity,
    queryFn: async () => {
      return await Object.keys(ClashCores).reduce(
        async (acc, key) => {
          const result = await acc
          try {
            const currentVersion =
              unwrapResult(
                await invokeQuery(
                  api.queries.getCoreVersion(key as ClashCore_Deserialize),
                ),
              ) ?? 'N/A'

            result[key as ClashCore_Serialize] = {
              name: ClashCores[key as ClashCore_Serialize],
              currentVersion,
            }
          } catch (e) {
            console.error('failed to fetch core version', e)
            result[key as ClashCore_Serialize] = {
              name: ClashCores[key as ClashCore_Serialize],
              currentVersion: 'N/A',
            }
          }
          return result
        },
        Promise.resolve({} as ClashCoresInfo),
      )
    },
  })

  const fetchRemote = useMutation({
    mutationKey: fetchLatestCoreVersions.queryKey,
    mutationFn: async () => {
      const results = unwrapResult(await invokeQuery(fetchLatestCoreVersions))

      if (!results) {
        return
      }

      const currentData = queryClient.getQueryData(
        coreQueryKey,
      ) as ClashCoresInfo

      if (currentData && results) {
        const updatedData = { ...currentData }

        Object.entries(results).forEach(([_key, latestVersion]) => {
          const key = kebabCase(_key)

          if (updatedData[key as ClashCore_Serialize]) {
            updatedData[key as ClashCore_Serialize] = {
              ...updatedData[key as ClashCore_Serialize],
              latestVersion,
            }
          }
        })

        queryClient.setQueryData(coreQueryKey, updatedData)
      }
      return results
    },
  })

  const updateCore = useMutation({
    mutationKey: updateCoreCommand.mutationKey,
    mutationFn: async (core: ClashCore_Deserialize) => {
      return unwrapResult(await invokeMutation(updateCoreCommand, [core]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: coreQueryKey })
    },
  })

  const upsert = useMutation({
    mutationKey: changeClashCoreCommand.mutationKey,
    mutationFn: async (core: ClashCore_Deserialize) => {
      return unwrapResult(await invokeMutation(changeClashCoreCommand, [core]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: coreQueryKey })
      queryClient.invalidateQueries({
        queryKey: api.queries.getAppConfig().queryKey,
      })
      queryClient.invalidateQueries({
        queryKey: api.queries.clashApiGetVersion().queryKey,
      })
    },
  })

  const restartSidecar = async () => {
    return await invokeMutation(restartSidecarCommand, [])
  }

  const inspectUpdater = async (updaterId: number) => {
    return unwrapResult(
      await invokeQuery(api.queries.inspectUpdater(updaterId)),
    )
  }

  // An update replaces the binary after `updateCore` resolves, so its caller
  // refetches once the updater is done.
  const refetchVersions = () =>
    queryClient.invalidateQueries({ queryKey: coreQueryKey })

  return {
    query,
    updateCore,
    inspectUpdater,
    upsert,
    restartSidecar,
    fetchRemote,
    refetchVersions,
  }
}

/** The installed version of one core, for views that show only that core. */
export const useClashCoreVersion = (core?: ClashCore_Serialize | null) => {
  const api = useQueryApi()
  const options = api.queries.getCoreVersion(core as ClashCore_Deserialize)
  return useQuery({
    queryKey: options.queryKey,
    queryFn: async () => unwrapResult(await invokeQuery(options)) ?? 'N/A',
    enabled: !!core,
    // Shares the invalidation of `useClashCores`, whose key prefixes this one.
    staleTime: Infinity,
  })
}
