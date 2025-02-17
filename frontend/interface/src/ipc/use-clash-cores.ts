import { kebabCase } from 'lodash-es'
import { unwrapResult } from '@/utils'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { commands, type ClashCore } from './bindings'
import { CLASH_VERSION_QUERY_KEY } from './consts'

export const ClashCores = {
  clash: 'Clash Premium',
  mihomo: 'Mihomo',
  'mihomo-alpha': 'Mihomo Alpha',
  'clash-rs': 'Clash Rust',
  'clash-rs-alpha': 'Clash Rust Alpha',
} as Record<ClashCore, string>

export type ClashCoresInfo = Record<ClashCore, ClashCoresDetail>

export type ClashCoresDetail = {
  name: string
  currentVersion: string
  latestVersion?: string
}

export const useClashCores = () => {
  const queryClient = useQueryClient()

  const query = useQuery({
    queryKey: ['clash-core'],
    queryFn: async () => {
      return await Object.keys(ClashCores).reduce(
        async (acc, key) => {
          const result = await acc
          try {
            const currentVersion =
              unwrapResult(await commands.getCoreVersion(key as ClashCore)) ??
              'N/A'

            result[key as ClashCore] = {
              name: ClashCores[key as ClashCore],
              currentVersion,
            }
          } catch (e) {
            console.error('failed to fetch core version', e)
            result[key as ClashCore] = {
              name: ClashCores[key as ClashCore],
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
    mutationFn: async () => {
      const results = unwrapResult(await commands.fetchLatestCoreVersions())

      if (!results) {
        return
      }

      const currentData = queryClient.getQueryData([
        'clash-core',
      ]) as ClashCoresInfo

      if (currentData && results) {
        const updatedData = { ...currentData }

        Object.entries(results).forEach(([_key, latestVersion]) => {
          const key = kebabCase(_key)

          if (updatedData[key as ClashCore]) {
            updatedData[key as ClashCore] = {
              ...updatedData[key as ClashCore],
              latestVersion,
            }
          }
        })

        queryClient.setQueryData(['clash-core'], updatedData)
      }
      return results
    },
  })

  const updateCore = useMutation({
    mutationFn: async (core: ClashCore) => {
      return unwrapResult(await commands.updateCore(core))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['clash-core'] })
    },
  })

  const upsert = useMutation({
    mutationFn: async (core: ClashCore) => {
      return unwrapResult(await commands.changeClashCore(core))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['clash-core'] })
      queryClient.invalidateQueries({ queryKey: ['settings'] })
      queryClient.invalidateQueries({ queryKey: [CLASH_VERSION_QUERY_KEY] })
    },
  })

  const restartSidecar = async () => {
    return await commands.restartSidecar()
  }

  return {
    query,
    updateCore,
    upsert,
    restartSidecar,
    fetchRemote,
  }
}
