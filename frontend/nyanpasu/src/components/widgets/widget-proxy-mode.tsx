import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import {
  MutationUnconfirmedError,
  useCoreStatus,
  useProxyMode,
  type ProxyMode,
} from '@nyanpasu/query'
import type { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId, type WidgetConfig } from './widget-config'
import WidgetItem from './widget-item'

const MODE_KEYS: ProxyMode[] = ['rule', 'global', 'direct', 'script']

function modeLabel(mode: ProxyMode): string {
  switch (mode) {
    case 'rule':
      return m.dashboard_widget_proxy_mode_rule()
    case 'global':
      return m.dashboard_widget_proxy_mode_global()
    case 'direct':
      return m.dashboard_widget_proxy_mode_direct()
    case 'script':
      return m.dashboard_widget_proxy_mode_script()
  }
}

function ProxyModePreview({ id }: { id: string }) {
  return (
    <WidgetItem id={id} widgetType={WidgetId.ProxyMode} minW={4} minH={2}>
      <Card className="flex size-full flex-col">
        <CardHeader className="shrink-0 text-base font-medium">
          {m.dashboard_widget_proxy_mode_title()}
        </CardHeader>
        <CardContent className="min-h-0 flex-1 justify-center">
          <div className="bg-surface-variant h-9 w-full animate-pulse rounded-full" />
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function ProxyModeLive({
  id,
  onCloseClick,
  disabled,
  config,
}: WidgetComponentProps & {
  disabled: boolean
  config: Extract<WidgetConfig, { type: WidgetId.ProxyMode }>
}) {
  const proxyMode = useProxyMode()
  const coreStatus = useCoreStatus()
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const configData = proxyMode.query.data
  const core = proxyMode.settingsQuery.data?.core
  const rawMode = configData?.mode?.toLowerCase()
  const supportedModes = core === 'clash' ? MODE_KEYS : MODE_KEYS.slice(0, 3)
  const currentMode = supportedModes.find((mode) => mode === rawMode) ?? null
  const waiting =
    proxyMode.query.isPending ||
    proxyMode.settingsQuery.isPending ||
    coreStatus.isPending
  const isOffline =
    coreStatus.data !== undefined &&
    coreStatus.data !== null &&
    coreStatus.data.status !== 'Running'
  const loadFailed =
    (proxyMode.query.isError ||
      proxyMode.settingsQuery.isError ||
      coreStatus.isError) &&
    !waiting
  const actionable =
    !disabled &&
    (actionState !== 'unconfirmed' || checkedAfterUnconfirmed) &&
    !checkingStatus &&
    !waiting &&
    !loadFailed &&
    !isOffline &&
    configData !== undefined &&
    proxyMode.settingsQuery.data !== undefined &&
    coreStatus.data?.status === 'Running'

  const changeMode = async (mode: ProxyMode) => {
    if (!actionable || proxyMode.isPending || !supportedModes.includes(mode))
      return
    setActionState('idle')
    setCheckedAfterUnconfirmed(false)
    setCheckFailed(false)
    try {
      const outcome = await proxyMode.upsert(mode)
      setActionState(
        outcome.status === 'committed_degraded' ? 'degraded' : 'idle',
      )
    } catch (error) {
      if (error instanceof MutationUnconfirmedError) {
        setActionState('unconfirmed')
        setCheckingStatus(true)
        try {
          await proxyMode.query.refetch()
        } catch {
          setCheckFailed(false)
        } finally {
          setCheckingStatus(false)
        }
      } else {
        setActionState('failed')
      }
    }
  }

  const checkStatus = async () => {
    if (disabled || checkingStatus) return
    setCheckingStatus(true)
    setCheckFailed(false)
    try {
      const result = await proxyMode.query.refetch()
      if (result.isError || result.data == null) {
        setCheckFailed(true)
        setCheckedAfterUnconfirmed(false)
      } else {
        setCheckedAfterUnconfirmed(true)
      }
    } catch {
      setCheckFailed(true)
      setCheckedAfterUnconfirmed(false)
    } finally {
      setCheckingStatus(false)
    }
  }

  const message = waiting
    ? m.dashboard_widget_proxy_mode_loading()
    : isOffline
      ? m.dashboard_widget_proxy_mode_core_offline()
      : loadFailed
        ? m.dashboard_widget_proxy_mode_load_failed()
        : currentMode
          ? null
          : m.dashboard_widget_proxy_mode_unknown()

  return (
    <WidgetItem
      id={id}
      widgetType={WidgetId.ProxyMode}
      minW={4}
      minH={2}
      onCloseClick={onCloseClick}
    >
      <Card className="flex size-full flex-col" data-slot="proxy-mode-card">
        <CardHeader className="shrink-0 gap-1 pt-3">
          <span className="text-base font-medium">
            {m.dashboard_widget_proxy_mode_title()}
          </span>
          {message && (
            <span
              className={
                loadFailed || isOffline
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={loadFailed || isOffline ? 'alert' : 'status'}
            >
              {message}
            </span>
          )}
        </CardHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-2 overflow-y-auto py-2">
          <SegmentedButton
            value={currentMode ?? ''}
            disabled={!actionable || proxyMode.isPending}
            aria-label={m.dashboard_widget_proxy_mode_title()}
            onValueChange={async (value) => {
              if (MODE_KEYS.includes(value as ProxyMode)) {
                await changeMode(value as ProxyMode)
              }
            }}
            data-slot="proxy-mode-options"
          >
            {supportedModes.map((mode) => (
              <SegmentedButtonItem
                key={mode}
                value={mode}
                disabled={
                  !actionable || proxyMode.isPending || mode === currentMode
                }
                hideIndicator
              >
                {modeLabel(mode)}
              </SegmentedButtonItem>
            ))}
          </SegmentedButton>

          {config.showHelp && (
            <p className="text-on-surface-variant text-center text-xs">
              {m.dashboard_widget_proxy_mode_help()}
            </p>
          )}

          {actionState === 'unconfirmed' && (
            <>
              <Button
                variant="basic"
                className="h-7 min-w-0 self-center px-2 text-xs"
                disabled={disabled || checkingStatus}
                loading={checkingStatus}
                onClick={checkStatus}
              >
                {m.dashboard_widget_operation_check()}
              </Button>
              {checkFailed && (
                <p className="text-error text-center text-xs" role="alert">
                  {m.dashboard_widget_operation_check_failed()}
                </p>
              )}
            </>
          )}

          {actionState !== 'idle' && (
            <p
              className={
                actionState === 'failed'
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={actionState === 'failed' ? 'alert' : 'status'}
            >
              {actionState === 'degraded'
                ? m.dashboard_widget_proxy_mode_committed_degraded()
                : actionState === 'unconfirmed'
                  ? m.dashboard_widget_proxy_mode_unconfirmed()
                  : m.dashboard_widget_proxy_mode_change_failed()}
            </p>
          )}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

export function ProxyModeWidget(props: WidgetComponentProps) {
  const { sourceOnly, isOverlay, disabled } = useDndGridContext()
  const config = useWidgetConfig(props.id, WidgetId.ProxyMode)

  if (sourceOnly || isOverlay) return <ProxyModePreview id={props.id} />

  return <ProxyModeLive {...props} disabled={!disabled} config={config} />
}
