import { useEffect } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { ErrorMessage } from '@/components/error-message'
import { m } from '@/paraglide/messages'
import { commands, events } from '@/services/rpc'
import { effectFailureMessage } from '@/utils/ipc-error'
import {
  acceptConfigurationStatus,
  attentionSources,
  invokeMutation,
  MutationUnconfirmedError,
  sourceMessage,
} from '@nyanpasu/query'
import { unwrapResult } from '@nyanpasu/rpc'
import {
  type ConfigurationStatus,
  type ConvergenceHealth,
  type EffectKind,
} from '@nyanpasu/rpc/types'
import {
  useMutation,
  useMutationState,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../_modules/settings-card'

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
    core_log_level: m.settings_clash_settings_log_level_label,
    core_log_storage: m.configuration_core_log_storage,
    transparent_proxy: m.configuration_transparent_proxy,
    widget: m.settings_nyanpasu_network_statistic_widget_label,
    tray: m.configuration_tray,
  }
  return labels[kind]()
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

const STATUS_KEY = ['getConfigurationStatus']

const StatusItem = ({
  label,
  health,
  summary,
  message,
  retry,
  busy,
}: {
  label: string
  health: ConvergenceHealth
  /** The localized reason, when the failure has a code. */
  summary?: string | null
  /** The owner's diagnostic text, as reported. */
  message?: string | null
  retry?: () => void
  busy?: boolean
}) => (
  <SettingsCard>
    <SettingsCardContent>
      <ItemContainer>
        <ItemLabel className="min-w-0">
          <ItemLabelText>{label}</ItemLabelText>

          <ItemLabelDescription>{healthLabel(health)}</ItemLabelDescription>

          {summary && <ItemLabelDescription>{summary}</ItemLabelDescription>}

          {message && (
            <ItemLabelDescription className="break-all">
              {message}
            </ItemLabelDescription>
          )}
        </ItemLabel>

        {retry && (
          <Button variant="stroked" disabled={busy} onClick={retry}>
            {m.configuration_retry()}
          </Button>
        )}
      </ItemContainer>
    </SettingsCardContent>
  </SettingsCard>
)

export default function ConfigurationStatus() {
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
  const error = retry.error ? (
    retry.error instanceof MutationUnconfirmedError ? (
      m.configuration_unconfirmed()
    ) : (
      <ErrorMessage error={retry.error} />
    )
  ) : isError ? (
    m.configuration_unconfirmed()
  ) : undefined

  const sources = status ? attentionSources(status) : []
  const operations = status?.recent_operations.slice(0, 8) ?? []

  return (
    <div data-slot="configuration-status-container" aria-live="polite">
      <SettingsLabel>{m.configuration_status()}</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent className="flex flex-col gap-2 text-sm">
            <p className="text-on-surface-variant">
              {m.configuration_committed_note()}
            </p>

            {unconfirmed && (
              <p className="text-error" role="status">
                {m.configuration_unconfirmed()}
              </p>
            )}

            {error && (
              <p className="text-error" role="status">
                {error}
              </p>
            )}

            {!status && <p>{m.configuration_pending()}</p>}

            {status?.maintenance && <p>{status.maintenance}</p>}
          </SettingsCardContent>
        </SettingsCard>

        {status && (
          <StatusItem
            label={m.configuration_runtime()}
            health={status.runtime.health}
            message={status.runtime.message}
            retry={
              status.runtime.health !== 'healthy' || status.maintenance
                ? () => retry.mutate(undefined)
                : undefined
            }
            busy={busy}
          />
        )}

        {status?.effects.map((effect) => (
          <StatusItem
            key={effect.kind}
            label={effectLabel(effect.kind)}
            health={effect.health}
            summary={effect.code && effectFailureMessage(effect.code)}
            message={effect.message}
            retry={
              effect.health !== 'healthy'
                ? () => retry.mutate(effect.kind)
                : undefined
            }
            busy={busy}
          />
        ))}
      </SettingsGroup>

      {sources.length > 0 && (
        <>
          <SettingsLabel>{m.configuration_sources()}</SettingsLabel>

          <SettingsGroup>
            {sources.map((source) => (
              <StatusItem
                key={source.profile}
                label={source.name}
                health={source.health}
                message={sourceMessage(source)}
              />
            ))}
          </SettingsGroup>
        </>
      )}

      {status && (status.active || operations.length > 0) && (
        <>
          <SettingsLabel>{m.configuration_recent_operations()}</SettingsLabel>

          <SettingsCard>
            <SettingsCardContent className="flex flex-col gap-3 text-sm">
              {status.active && (
                <p>
                  {m.configuration_pending()}: {status.active}
                </p>
              )}

              {operations.map((operation) => (
                <div key={operation.operation_id} className="break-all">
                  <code>{operation.operation_id}</code>

                  <p className="text-on-surface-variant">
                    {operation.domain} / {operation.outcome} /{' '}
                    {operation.conclusion}
                  </p>

                  {operation.message && <p>{operation.message}</p>}
                </div>
              ))}
            </SettingsCardContent>
          </SettingsCard>
        </>
      )}
    </div>
  )
}
