import { unwrapResult, type Result } from '@nyanpasu/rpc'
import type {
  ClashConfig,
  ClashConfigPatch_Deserialize,
  IpcError,
  MutationOutcome,
  NyanpasuAppConfig_Serialize,
  NyanpasuAppConfigPatch_Serialize,
} from '@nyanpasu/rpc/types'
import {
  useMutation,
  useQuery,
  useQueryClient,
  type MutationKey,
  type QueryKey,
} from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, unwrapQueryOptions } from './query-options'

type ConfigQuery<TConfig> = {
  queryKey: QueryKey
  queryFn?: (
    context: never,
  ) => Result<TConfig, IpcError> | Promise<Result<TConfig, IpcError>>
}

type ConfigMutation<TPatch> = {
  mutationKey?: MutationKey
  mutationFn?: (
    input: [TPatch],
    context: never,
  ) => Promise<Result<MutationOutcome<null>, IpcError>>
}

const useTypedConfig = <TConfig, TPatch>(
  configQuery: ConfigQuery<TConfig>,
  patchConfig: ConfigMutation<TPatch>,
) => {
  const queryClient = useQueryClient()

  const query = useQuery(unwrapQueryOptions(configQuery, configQuery.queryFn!))

  const upsert = useMutation({
    mutationKey: patchConfig.mutationKey,
    // Returns the whole MutationOutcome so the MutationCache can see
    // `committed_degraded`: a settings change whose side effects degraded is
    // still committed, so it stays on the success path. Do not collapse it to
    // a bare value.
    mutationFn: async (patch: TPatch) => {
      return unwrapResult(await invokeMutation(patchConfig, [patch]))
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

const useTypedConfigField = <
  TConfig,
  TPatch,
  K extends keyof TConfig & keyof TPatch,
  TClearable extends keyof TPatch = never,
>(
  { query, upsert: update }: ReturnType<typeof useTypedConfig<TConfig, TPatch>>,
  key: K,
) => {
  // Read only what is returned: React Query re-renders an observer for each
  // result field it read, so enumerating the result (a rest spread) made every
  // consumer re-render on each fetch start and end.
  const data = query.data
  const value = data?.[key]

  /**
   * A composite field (for example `mixed_port`) takes a nested patch: pass
   * only the sub-fields to change. Only a `TClearable` field (for example
   * `socks_port`) takes `null`, which clears it; any other field reads JSON
   * `null` as "not set", so the patch would silently change nothing.
   * Does nothing until the config has loaded.
   */
  const upsert = async (
    value: K extends TClearable
      ? Exclude<TPatch[K], undefined>
      : NonNullable<TPatch[K]>,
  ) => {
    if (!data) {
      return
    }

    await update.mutateAsync({ [key]: value } as TPatch)
  }

  return {
    value,
    upsert,
    /** Whether a patch is in flight. */
    isPending: update.isPending,
    refetch: query.refetch,
  }
}

/**
 * The application config (`NyanpasuAppConfig`).
 *
 * @example
 * ```tsx
 * const { query, upsert } = useSettings()
 *
 * upsert.mutate({ theme_mode: 'dark' })
 * ```
 */
export const useSettings = () => {
  const api = useQueryApi()
  return useTypedConfig<
    NyanpasuAppConfig_Serialize,
    NyanpasuAppConfigPatch_Serialize
  >(api.queries.getAppConfig(), api.mutations.patchAppConfig)
}

/**
 * One field of the application config, addressed by its typed name.
 *
 * @example
 * ```tsx
 * const { value, upsert } = useSetting('theme_mode')
 * ```
 */
export const useSetting = <
  K extends keyof NyanpasuAppConfig_Serialize &
    keyof NyanpasuAppConfigPatch_Serialize,
>(
  key: K,
) => {
  return useTypedConfigField(useSettings(), key)
}

/**
 * The persistent clash config (`ClashConfig`), not the live core `/configs`.
 */
export const useClashSettings = () => {
  const api = useQueryApi()
  return useTypedConfig<ClashConfig, ClashConfigPatch_Deserialize>(
    api.queries.getClashConfig(),
    api.mutations.patchClashConfig,
  )
}

/**
 * The clash fields whose patch is a double option, so `null` clears them. The
 * generated patch type cannot tell them apart from the other fields.
 */
type ClearableClashField = 'socks_port' | 'http_port'

/**
 * One field of the persistent clash config, addressed by its typed name.
 *
 * @example
 * ```tsx
 * const { value, upsert } = useClashSetting('enable_tun_mode')
 * ```
 */
export const useClashSetting = <
  K extends keyof ClashConfig & keyof ClashConfigPatch_Deserialize,
>(
  key: K,
) => {
  return useTypedConfigField<
    ClashConfig,
    ClashConfigPatch_Deserialize,
    K,
    ClearableClashField
  >(useClashSettings(), key)
}
