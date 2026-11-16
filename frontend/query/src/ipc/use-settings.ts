import { unwrapResult, type Result } from '@nyanpasu/rpc'
import type {
  ApplicationPreferencesPatch_Serialize,
  ClashConfig,
  ClashSettingsPatch_Deserialize,
  ClashSystemPatch_Deserialize,
  IpcError,
  MutationOutcome,
  NyanpasuAppConfig_Serialize,
  SystemSettingsPatch_Deserialize,
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
    // Returning the refetch keeps the patch pending until the config is read
    // back, so the next patch is built from saved values, not stale ones.
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: configQuery.queryKey,
      }),
  })

  return {
    query,
    upsert,
  }
}

const useTypedConfigField = <
  TConfig,
  TPatch,
  K extends keyof TConfig,
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
    value: K extends keyof TPatch
      ? K extends TClearable
        ? Exclude<TPatch[K], undefined>
        : NonNullable<TPatch[K]>
      : never,
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
 * Ordinary application preferences only (`settings` scope). System-affecting
 * fields use `useSystemSettings`; host-bound hotkeys, service mode and the core
 * selection use their own commands. Reads keep the full application config.
 * Keep nested patches optional when they are untouched.
 */
type AppConfigPatch = Partial<ApplicationPreferencesPatch_Serialize>

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
  return useTypedConfig<NyanpasuAppConfig_Serialize, AppConfigPatch>(
    api.queries.getAppConfig(),
    api.mutations.patchAppConfig,
  )
}

/**
 * One field of the application config, addressed by its typed name.
 *
 * @example
 * ```tsx
 * const { value, upsert } = useSetting('theme_mode')
 * ```
 */
export const useSetting = <K extends keyof NyanpasuAppConfig_Serialize>(
  key: K,
) => {
  return useTypedConfigField(useSettings(), key)
}

/**
 * The system-affecting application settings (`system` scope): autostart, the
 * system proxy with its guard and bypass list, and the PAC URL. They read from
 * the same application config as `useSettings`.
 *
 * @example
 * ```tsx
 * const { query, upsert } = useSystemSettings()
 *
 * upsert.mutate({ enable_system_proxy: true })
 * ```
 */
export const useSystemSettings = () => {
  const api = useQueryApi()
  return useTypedConfig<
    NyanpasuAppConfig_Serialize,
    SystemSettingsPatch_Deserialize
  >(api.queries.getAppConfig(), api.mutations.patchSystemSettings)
}

/**
 * One system-affecting application setting, addressed by its typed name.
 * `pac_url` is the clearable one: `null` clears it.
 *
 * @example
 * ```tsx
 * const { value, upsert } = useSystemSetting('enable_system_proxy')
 * ```
 */
export const useSystemSetting = <
  K extends keyof NyanpasuAppConfig_Serialize &
    keyof SystemSettingsPatch_Deserialize,
>(
  key: K,
) => {
  return useTypedConfigField<
    NyanpasuAppConfig_Serialize,
    SystemSettingsPatch_Deserialize,
    K,
    'pac_url'
  >(useSystemSettings(), key)
}

/**
 * The persistent clash config (`ClashConfig`), not the live core `/configs`.
 * Writes only the ordinary settings (`settings` scope); the control plane uses
 * `useClashSystemSettings`.
 */
export const useClashSettings = () => {
  const api = useQueryApi()
  return useTypedConfig<ClashConfig, ClashSettingsPatch_Deserialize>(
    api.queries.getClashConfig(),
    api.mutations.patchClashConfig,
  )
}

/**
 * One field of the persistent clash config, addressed by its typed name. Any
 * field can be read; `upsert` accepts only the ordinary fields, and is typed
 * `never` for a control-plane field (use `useClashSystemSetting` to write it).
 *
 * @example
 * ```tsx
 * const { value, upsert } = useClashSetting('expand_include_all')
 * ```
 */
export const useClashSetting = <K extends keyof ClashConfig>(key: K) => {
  return useTypedConfigField<ClashConfig, ClashSettingsPatch_Deserialize, K>(
    useClashSettings(),
    key,
  )
}

/**
 * The persistent clash config, written through the control-plane request
 * (`system` scope): controller, ports, TUN, transparent capture, web UI
 * templates and the field filter.
 */
export const useClashSystemSettings = () => {
  const api = useQueryApi()
  return useTypedConfig<ClashConfig, ClashSystemPatch_Deserialize>(
    api.queries.getClashConfig(),
    api.mutations.patchClashSystemConfig,
  )
}

/**
 * The clash fields whose patch is a double option, so `null` clears them. The
 * generated patch type cannot tell them apart from the other fields.
 */
type ClearableClashField =
  'socks_port' | 'http_port' | 'redir_port' | 'tproxy_port'

/**
 * One control-plane field of the persistent clash config, addressed by its
 * typed name.
 *
 * @example
 * ```tsx
 * const { value, upsert } = useClashSystemSetting('enable_tun_mode')
 * ```
 */
export const useClashSystemSetting = <
  K extends keyof ClashConfig & keyof ClashSystemPatch_Deserialize,
>(
  key: K,
) => {
  return useTypedConfigField<
    ClashConfig,
    ClashSystemPatch_Deserialize,
    K,
    ClearableClashField
  >(useClashSystemSettings(), key)
}
