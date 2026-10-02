import ErrorOutlineRounded from '~icons/material-symbols/error-outline-rounded'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { m } from '@/paraglide/messages'
import { effectFailureMessage } from '@/utils/ipc-error'
import {
  attentionSources,
  MutationUnconfirmedError,
  sourceMessage,
  useConfigurationStatus,
} from '@nyanpasu/query'
import type {
  ConfigurationStatus,
  ConvergenceHealth,
  EffectKind,
} from '@nyanpasu/rpc/types'
import { Link } from '@tanstack/react-router'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'

type HealthRow = {
  key: string
  label: string
  health: ConvergenceHealth | 'maintenance'
  message?: string | null
  retry?: () => void
  priority: number
}

function effectLabel(kind: EffectKind) {
  const labels: Record<EffectKind, () => string> = {
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

function healthLabel(health: ConvergenceHealth | 'maintenance') {
  if (health === 'maintenance')
    return m.dashboard_widget_configuration_health_maintenance()
  const labels: Record<ConvergenceHealth, () => string> = {
    healthy: m.configuration_healthy,
    pending: m.configuration_pending,
    retry_scheduled: m.configuration_retry_scheduled,
    waiting_dependency: m.configuration_waiting_dependency,
    blocked: m.configuration_blocked,
    recovery_required: m.configuration_recovery_required,
  }
  return labels[health]()
}

function issueRows(
  status: ConfigurationStatus,
  showSources: boolean,
  retryRuntime: () => void,
  retryEffect: (kind: EffectKind) => void,
) {
  const rows: HealthRow[] = []
  if (status.maintenance) {
    rows.push({
      key: 'maintenance',
      label: m.dashboard_widget_configuration_health_maintenance(),
      health: 'maintenance',
      message: status.maintenance,
      priority: 0,
    })
  }
  if (status.runtime.health !== 'healthy') {
    rows.push({
      key: 'runtime',
      label: m.configuration_runtime(),
      health: status.runtime.health,
      message: status.runtime.message,
      retry: retryRuntime,
      priority:
        status.runtime.health === 'recovery_required' ||
        status.runtime.health === 'blocked'
          ? 1
          : 2,
    })
  }
  for (const effect of status.effects) {
    if (effect.health === 'healthy') continue
    rows.push({
      key: `effect:${effect.kind}`,
      label: effectLabel(effect.kind),
      health: effect.health,
      message:
        (effect.code && effectFailureMessage(effect.code)) || effect.message,
      retry: () => retryEffect(effect.kind),
      priority:
        effect.health === 'recovery_required' || effect.health === 'blocked'
          ? 1
          : 3,
    })
  }
  if (showSources) {
    for (const source of attentionSources(status)) {
      rows.push({
        key: `source:${source.profile}`,
        label: source.name,
        health: source.health,
        message: sourceMessage(source),
        priority: 4,
      })
    }
  }
  return rows.sort((left, right) => left.priority - right.priority)
}

function ConfigurationHealthPreview({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ConfigurationHealth}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-configuration-health-card">
        <CardContent className="flex size-full flex-col gap-3">
          <div className="flex items-center gap-2 font-bold">
            <ErrorOutlineRounded className="size-5" />
            {m.dashboard_widget_configuration_health_title()}
          </div>
          <p className="text-on-surface-variant text-sm">
            {m.dashboard_widget_configuration_health_preview()}
          </p>
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

function ConfigurationHealthLive({
  id,
  onCloseClick,
  canAct,
  sizeLimit,
}: WidgetComponentProps & { canAct: boolean; sizeLimit: 1 | 3 | 5 }) {
  const config = useWidgetConfig(id, WidgetId.ConfigurationHealth)
  const {
    status,
    isLoading,
    isError,
    refetch,
    retryRuntime,
    retryEffect,
    isRetrying,
    retryError,
    isLatestOperationUnconfirmed,
  } = useConfigurationStatus()

  const rows = status
    ? issueRows(
        status,
        config.showSources,
        () => {
          retryRuntime().catch(() => undefined)
        },
        (kind) => {
          retryEffect(kind).catch(() => undefined)
        },
      )
    : []
  const visibleRows = rows.slice(0, Math.min(config.maxItems, sizeLimit))
  const unconfirmed =
    retryError instanceof MutationUnconfirmedError ||
    isLatestOperationUnconfirmed
  const handleRetryRead = () => {
    refetch().catch(() => undefined)
  }

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ConfigurationHealth}
      minW={3}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="size-full" data-slot="widget-configuration-health-card">
        <CardContent className="flex size-full min-h-0 flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2 font-bold">
              <ErrorOutlineRounded className="size-5 shrink-0" />
              <span className="truncate">
                {m.dashboard_widget_configuration_health_title()}
              </span>
            </div>
            <Link
              aria-disabled={!canAct}
              tabIndex={canAct ? 0 : -1}
              onClick={(event) => {
                if (!canAct) event.preventDefault()
              }}
              className={`text-primary shrink-0 text-xs hover:underline ${!canAct ? 'pointer-events-none opacity-50' : ''}`}
              to="/main/settings/debug"
            >
              {m.dashboard_widget_configuration_health_details()}
            </Link>
          </div>

          {isLoading && !status ? (
            <p className="text-on-surface-variant text-sm" role="status">
              {m.dashboard_widget_configuration_health_loading()}
            </p>
          ) : isError && !status ? (
            <div className="flex min-h-0 flex-1 flex-col justify-center gap-2">
              <p className="text-error text-sm" role="alert">
                {m.dashboard_widget_configuration_health_read_failed()}
              </p>
              <Button
                className="h-8 self-start px-3"
                disabled={!canAct}
                onClick={handleRetryRead}
              >
                {m.dashboard_widget_configuration_health_retry_read()}
              </Button>
            </div>
          ) : (
            <>
              {isError && (
                <p className="text-error text-xs" role="status">
                  {m.dashboard_widget_configuration_health_stale()}
                </p>
              )}
              {unconfirmed && (
                <div className="flex items-center gap-2">
                  <p className="text-error text-xs" role="status">
                    {m.dashboard_widget_configuration_health_unconfirmed()}
                  </p>
                  {retryError instanceof MutationUnconfirmedError && (
                    <Button
                      className="h-7 min-w-0 shrink-0 px-2 text-xs"
                      disabled={!canAct || isRetrying}
                      onClick={handleRetryRead}
                    >
                      {m.dashboard_widget_configuration_health_retry_read()}
                    </Button>
                  )}
                </div>
              )}
              {retryError && !unconfirmed && (
                <p className="text-error text-xs" role="status">
                  {m.dashboard_widget_configuration_health_retry_failed()}
                </p>
              )}
              {visibleRows.length === 0 ? (
                <p
                  className="text-on-surface-variant flex-1 text-sm"
                  role="status"
                >
                  {isError
                    ? m.dashboard_widget_configuration_health_stale()
                    : m.dashboard_widget_configuration_health_healthy()}
                </p>
              ) : (
                <>
                  <p className="text-on-surface-variant text-xs">
                    {rows.length}{' '}
                    {m.dashboard_widget_configuration_health_attention()}
                  </p>
                  <ul className="min-h-0 flex-1 space-y-1 overflow-auto">
                    {visibleRows.map((row) => {
                      const handleRetry = () => row.retry?.()
                      return (
                        <li
                          className="hover:bg-surface-variant/40 flex min-w-0 items-center gap-2 rounded-xl px-2 py-1 text-xs"
                          key={row.key}
                        >
                          <div className="min-w-0 flex-1">
                            <div className="truncate font-medium">
                              {row.label}
                            </div>
                            <div className="text-on-surface-variant truncate">
                              {row.message || healthLabel(row.health)}
                            </div>
                          </div>
                          {row.retry && (
                            <Button
                              className="h-7 min-w-0 shrink-0 px-2 text-xs"
                              disabled={
                                !canAct ||
                                isRetrying ||
                                retryError instanceof MutationUnconfirmedError
                              }
                              onClick={handleRetry}
                            >
                              {m.configuration_retry()}
                            </Button>
                          )}
                        </li>
                      )
                    })}
                  </ul>
                </>
              )}
            </>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

export function ConfigurationHealthWidget(props: WidgetComponentProps) {
  const { disabled, displayItems, isOverlay, sourceOnly } = useDndGridContext()
  if (sourceOnly || isOverlay) return <ConfigurationHealthPreview {...props} />
  const item = displayItems.find((candidate) => candidate.id === props.id)
  const sizeLimit =
    (item?.w ?? 3) >= 6 && (item?.h ?? 2) >= 4
      ? 5
      : (item?.w ?? 3) >= 4 && (item?.h ?? 2) >= 3
        ? 3
        : 1
  return (
    <ConfigurationHealthLive
      {...props}
      canAct={disabled && !isOverlay}
      sizeLimit={sizeLimit}
    />
  )
}
