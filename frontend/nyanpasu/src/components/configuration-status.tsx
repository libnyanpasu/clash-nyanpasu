import { useEffect, useState } from 'react'
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
import { useMutationState } from '@tanstack/react-query'

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

export function ConfigurationStatusPanel() {
  const mutations = useMutationState({ select: (mutation) => mutation.state })
  const latest = mutations.toSorted((a, b) => b.submittedAt - a.submittedAt)[0]
  const unconfirmed = latest?.error instanceof MutationUnconfirmedError
  const [status, setStatus] = useState<ConfigurationStatus>()
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let disposed = false
    const accept = (next: ConfigurationStatus) => {
      if (disposed) return
      setStatus((previous) => acceptConfigurationStatus(previous, next))
      setError(undefined)
    }
    const read = () =>
      commands
        .getConfigurationStatus()
        .then(accept)
        .catch(() => {
          if (!disposed) setError(m.configuration_unconfirmed())
        })
    const listener = events.configurationStatusChanged
      .listen((event) => accept(event.payload))
      .catch(() => undefined)
    read()
    const timer = window.setInterval(read, 5000)
    return () => {
      disposed = true
      window.clearInterval(timer)
      listener.then((unlisten) => unlisten?.())
    }
  }, [])

  const retry = async (kind?: EffectKind) => {
    setBusy(true)
    try {
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
      )
      const next = await commands.getConfigurationStatus()
      setStatus((previous) => acceptConfigurationStatus(previous, next))
      setError(undefined)
    } catch (error) {
      setError(
        error instanceof MutationUnconfirmedError
          ? m.configuration_unconfirmed()
          : String(error),
      )
    } finally {
      setBusy(false)
    }
  }
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
                  onClick={() => {
                    retry()
                  }}
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
                    onClick={() => {
                      retry(effect.kind)
                    }}
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
