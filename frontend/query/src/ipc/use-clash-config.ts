import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ClashAccessPatch_Deserialize,
  ClashApiConfig,
  ClashOverridesSettingsPatch_Deserialize,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'

/**
 * The live core configuration (`/configs`).
 *
 * `upsert` writes the ordinary runtime overrides (`settings` scope): log
 * level, IPv6, unified delay and TCP concurrency. The proxy mode and the
 * controller exposure have their own hooks, `useProxyMode` and
 * `useClashAccess`, because each needs a different scope.
 */
export const useClashConfig = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const configQuery = api.queries.clashApiGetConfigs()
  const patchConfig = api.mutations.patchRuntimeOverrides

  const query = useQuery<ClashApiConfig | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: ClashOverridesSettingsPatch_Deserialize) => {
      return unwrapResult(await invokeMutation(patchConfig, [payload]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: configQuery.queryKey,
      })
    },
  })

  return {
    query,
    upsert,
  }
}

/**
 * The controller exposure (`system` scope): whether the core accepts LAN
 * connections, and the secret that guards its controller. It reads the live
 * core configuration like `useClashConfig`.
 */
export const useClashAccess = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const configQuery = api.queries.clashApiGetConfigs()
  const patchConfig = api.mutations.patchRuntimeAccess

  const query = useQuery<ClashApiConfig | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: ClashAccessPatch_Deserialize) => {
      return unwrapResult(await invokeMutation(patchConfig, [payload]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: configQuery.queryKey,
      })
    },
  })

  return {
    query,
    upsert,
  }
}
