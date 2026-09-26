import { useEffect } from 'react'
import { m } from '@/paraglide/messages'
import {
  acceptConfigurationStatus,
  commands,
  events,
  invokeMutation,
  MutationUnconfirmedError,
  unwrapResult,
  type ConfigurationStatus,
  type ConvergenceHealth,
  type EffectKind,
} from '@nyanpasu/interface'
import {
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'

function healthLabel(health: ConvergenceHealth) {
  const labels = {
    healthy: m.configuration_healthy,
    pending: m.configuration_pending,
    retry_scheduled: m.configuration_retry_scheduled,
    waiting_dependency: m.configuration_waiting_dependency,
    blocked: m.configuration_blocked,
    recovery_required: m.configuration_recovery_required,
  }
  return labels[health]()
}

function effectLabel(kind: EffectKind) {
  const labels = {
    system_proxy: m.settings_system_proxy_system_proxy_label,
    proxy_guard: m.settings_system_proxy_proxy_guard_label,
    auto_launch: m.settings_system_proxy_auto_launch_label,
    hotkeys: m.configuration_hotkeys,
    locale: m.header_settings_action_language,
    logger: m.settings_nyanpasu_app_log_level_label,
    widget: m.settings_nyanpasu_network_statistic_widget_label,
    tray: m.configuration_tray,
  }
  return labels[kind]()
}

const CONFIGURATION_MUTATIONS = new Set<unknown>([
  'patchVergeConfig',
  'patchClashConfig',
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

const STATUS_KEY = ['getConfigurationStatus']

export function ConfigurationStatusPanel() {
  const queryClient = useQueryClient()
  const mutations = useMutationState({
    filters: {
      predicate: (mutation) =>
        CONFIGURATION_MUTATIONS.has(mutation.options.mutationKey?.[0]),
    },
    select: (mutation) => mutation.state,
  })
  let latest: (typeof mutations)[number] | undefined
  for (const mutation of mutations) {
    if (!latest || mutation.submittedAt > latest.submittedAt) latest = mutation
  }
  const unconfirmed = latest?.error instanceof MutationUnconfirmedError

  const { data: status, isError } = useQuery({
    queryKey: STATUS_KEY,
    // A late poll must not replace a newer event already in the cache.
    queryFn: async () => {
      const next = await commands.getConfigurationStatus()
      // Compare with the cache after the reply: an event may have landed meanwhile.
      return acceptConfigurationStatus(
        queryClient.getQueryData<ConfigurationStatus>(STATUS_KEY),
        next,
      )
    },
    refetchInterval: 10_000,
  })
  useEffect(() => {
    const listener = events.configurationStatusChanged
      .listen((event) =>
        queryClient.setQueryData<ConfigurationStatus>(STATUS_KEY, (previous) =>
          acceptConfigurationStatus(previous, event.payload),
        ),
      )
      .catch(() => undefined)
    return () => {
      listener.then((unlisten) => unlisten?.())
    }
  }, [queryClient])

  const retry = useMutation({
    mutationFn: async (kind?: EffectKind) =>
      unwrapResult(
        await invokeMutation(
          {
            mutationFn: () =>
              kind
                ? commands.retryConfigurationEffect(kind)
                : commands.retryConfigurationRuntime(),
          },
          undefined,
        ),
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STATUS_KEY }),
  })
  const busy = retry.isPending
  const error = retry.error
    ? retry.error instanceof MutationUnconfirmedError
      ? m.configuration_unconfirmed()
      : String(retry.error)
    : isError
      ? m.configuration_unconfirmed()
      : undefined

  const attention =
    status &&
    (status.maintenance ||
      status.runtime.health !== 'healthy' ||
      status.effects.some((effect) => effect.health !== 'healthy'))
  return (
    <details className="bg-background fixed right-4 bottom-4 z-40 max-w-md rounded-xl border p-3 text-sm shadow-lg">
      <summary className="cursor-pointer">
        {m.configuration_status()} {attention || unconfirmed ? '•' : ''}
      </summary>
      <div className="mt-3 max-h-80 space-y-3 overflow-auto" aria-live="polite">
        {unconfirmed && <p role="status">{m.configuration_unconfirmed()}</p>}
        {error && <p role="status">{error}</p>}
        {!status ? (
          <p>{m.configuration_pending()}</p>
        ) : (
          <>
            <p>{m.configuration_committed_note()}</p>
            {status.maintenance && <p>{status.maintenance}</p>}
            <div>
              <strong>{m.configuration_runtime()}</strong>:{' '}
              {healthLabel(status.runtime.health)}
              {status.runtime.message && <p>{status.runtime.message}</p>}
              {(status.runtime.health !== 'healthy' || status.maintenance) && (
                <button
                  className="underline"
                  disabled={busy}
                  onClick={() => retry.mutate(undefined)}
                >
                  {m.configuration_retry()}
                </button>
              )}
            </div>
            {status.effects.map((effect) => (
              <div key={effect.kind}>
                <span>
                  {effectLabel(effect.kind)}: {healthLabel(effect.health)}
                </span>
                {effect.message && <p>{effect.message}</p>}
                {effect.health !== 'healthy' && (
                  <button
                    className="ml-2 underline"
                    disabled={busy}
                    onClick={() => retry.mutate(effect.kind)}
                  >
                    {m.configuration_retry()}
                  </button>
                )}
              </div>
            ))}
            <details>
              <summary>{m.configuration_recent_operations()}</summary>
              {status.active && (
                <p>
                  {m.configuration_pending()}: {status.active}
                </p>
              )}
              {status.queued.map((id) => (
                <p key={id}>
                  {m.configuration_pending()}: {id}
                </p>
              ))}
              {status.recent_operations.slice(0, 8).map((operation) => (
                <div key={operation.operation_id} className="my-2 break-all">
                  <code>{operation.operation_id}</code>: {operation.domain} /{' '}
                  {operation.outcome} / {operation.conclusion}
                  {operation.message && <p>{operation.message}</p>}
                </div>
              ))}
            </details>
          </>
        )}
      </div>
    </details>
  )
}
