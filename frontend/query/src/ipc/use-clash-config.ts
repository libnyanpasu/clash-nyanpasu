import { unwrapResult } from '@nyanpasu/rpc'
import type {
  ClashAccessPatch_Deserialize,
  ClashApiSettings,
  ClashOverridesSettingsPatch_Deserialize,
} from '@nyanpasu/rpc/types'
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type QueryKey,
} from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'

/**
 * The live core configuration (`/configs`) without the controller secret
 * (`ClashApiSettings`), so ordinary readers need only `read`.
 *
 * `upsert` writes the ordinary runtime overrides (`settings` scope): log
 * level, IPv6, unified delay and TCP concurrency. The proxy mode and the
 * controller exposure have their own hooks, `useProxyMode` and
 * `useClashAccess`, because each needs a different scope.
 */
export const useClashConfig = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const configQuery = api.queries.clashApiGetSettings()
  const patchConfig = api.mutations.patchRuntimeOverrides

  const query = useQuery<ClashApiSettings | undefined>(
    unwrapQueryOptions(configQuery, configQuery.queryFn!),
  )

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: ClashOverridesSettingsPatch_Deserialize) => {
      return unwrapResult(await invokeMutation(patchConfig, [payload]))
    },
    onSuccess: () => {
      refreshLiveConfig(
        queryClient,
        api.queries.clashApiGetSettings().queryKey,
        api.queries.clashApiGetConfigs().queryKey,
      )
    },
  })

  return {
    query,
    upsert,
  }
}

/**
 * The controller exposure (`system` scope): whether the core accepts LAN
 * connections, and the secret that guards its controller. It only writes: the
 * full live configuration carries the secret and is `sensitive_read`, so the
 * LAN flag is read through `useClashConfig`.
 */
export const useClashAccess = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const patchConfig = api.mutations.patchRuntimeAccess

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    mutationFn: async (payload: ClashAccessPatch_Deserialize) => {
      return unwrapResult(await invokeMutation(patchConfig, [payload]))
    },
    onSuccess: () => {
      refreshLiveConfig(
        queryClient,
        api.queries.clashApiGetSettings().queryKey,
        api.queries.clashApiGetConfigs().queryKey,
      )
    },
  })

  return {
    upsert,
  }
}

/** The safe and the full live configuration are cached apart; refresh both. */
const refreshLiveConfig = (
  queryClient: QueryClient,
  ...queryKeys: readonly QueryKey[]
) =>
  Promise.all(
    queryKeys.map((queryKey) => queryClient.invalidateQueries({ queryKey })),
  )
