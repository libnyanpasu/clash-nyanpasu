import { kebabCase } from 'lodash-es'
import { unwrapResult } from '@interface/utils'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  mutations,
  queries,
  type ClashCore,
  type ClashCore_Deserialize,
  type ClashCore_Serialize,
} from './bindings'
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
  const queryClient = useQueryClient()
  const coreQueryKey = queries.getCoreVersion('clash').queryKey.slice(0, 1)
  const fetchLatestCoreVersions = queries.fetchLatestCoreVersions()
  const updateCoreCommand = mutations.updateCore
  const changeClashCoreCommand = mutations.changeClashCore
  const restartSidecarCommand = mutations.restartSidecar

  const query = useQuery({
    queryKey: coreQueryKey,
    queryFn: async () => {
      return await Object.keys(ClashCores).reduce(
        async (acc, key) => {
          const result = await acc
          try {
            const currentVersion =
              unwrapResult(
                await invokeQuery(
                  queries.getCoreVersion(key as ClashCore_Deserialize),
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
        queryKey: queries.getVergeConfig().queryKey,
      })
      queryClient.invalidateQueries({
        queryKey: queries.clashApiGetVersion().queryKey,
      })
    },
  })

  const restartSidecar = async () => {
    return await invokeMutation(restartSidecarCommand, [])
  }

  const inspectUpdater = async (updaterId: number) => {
    return unwrapResult(await invokeQuery(queries.inspectUpdater(updaterId)))
  }

  return {
    query,
    updateCore,
    inspectUpdater,
    upsert,
    restartSidecar,
    fetchRemote,
  }
}
