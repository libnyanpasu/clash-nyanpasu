import CodeRounded from '~icons/material-symbols/code-rounded'
import NorthEastRounded from '~icons/material-symbols/north-east-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { ExpressiveChoice } from '@nyanpasu/ui/expressive-choice'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import {
  MutationUnconfirmedError,
  useCoreStatus,
  useProxyMode,
  type ProxyMode,
} from '@nyanpasu/query'
import type { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem from './widget-item'
import { WidgetHeader, WidgetTitle } from './widget-ui'

const MODE_KEYS: ProxyMode[] = ['rule', 'global', 'direct', 'script']
const MODE_ICONS = {
  rule: RouteRounded,
  global: PublicRounded,
  direct: NorthEastRounded,
  script: CodeRounded,
}

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

function modeDescription(mode: ProxyMode): string {
  switch (mode) {
    case 'rule':
      return m.dashboard_widget_proxy_mode_rule_description()
    case 'global':
      return m.dashboard_widget_proxy_mode_global_description()
    case 'direct':
      return m.dashboard_widget_proxy_mode_direct_description()
    case 'script':
      return m.dashboard_widget_proxy_mode_script_description()
  }
}

function ProxyModeOptions({
  modes,
  currentMode,
  disabled,
  onChange,
  layout = 'focus',
}: {
  modes: ProxyMode[]
  currentMode: ProxyMode | null
  disabled: boolean
  onChange?: (mode: ProxyMode) => void
  layout?: 'focus' | 'flex'
}) {
  return (
    <ExpressiveChoice
      label={m.dashboard_widget_proxy_mode_title()}
      options={modes.map((mode) => {
        const Icon = MODE_ICONS[mode]
        return {
          value: mode,
          label: modeLabel(mode),
          description: modeDescription(mode),
          icon: <Icon aria-hidden />,
        }
      })}
      value={currentMode}
      layout={layout}
      disabled={disabled}
      onChange={(value) => {
        const mode = modes.find((mode) => mode === value)
        if (mode) onChange?.(mode)
      }}
    />
  )
}

function ProxyModePreview({ id }: { id: string }) {
  const config = useWidgetConfig(id, WidgetId.ProxyMode)
  return (
    <WidgetItem id={id} widgetType={WidgetId.ProxyMode} minW={4} minH={2}>
      <Card className="flex size-full flex-col">
        <WidgetHeader className="gap-2">
          <WidgetTitle icon={RouteRounded}>
            {m.dashboard_widget_proxy_mode_title()}
          </WidgetTitle>
        </WidgetHeader>
        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-hidden px-4 py-3">
          <ProxyModeOptions
            modes={MODE_KEYS.slice(0, 3)}
            currentMode="rule"
            disabled
            layout={config.layout}
          />
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
}: WidgetComponentProps & {
  disabled: boolean
}) {
  const proxyMode = useProxyMode()
  const config = useWidgetConfig(id, WidgetId.ProxyMode)
  const coreStatus = useCoreStatus()
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const configData = proxyMode.query.data
  const layout = config.layout
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
    if (
      !actionable ||
      proxyMode.isPending ||
      mode === currentMode ||
      !supportedModes.includes(mode)
    )
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
        <WidgetHeader className="flex-wrap gap-x-2 gap-y-1">
          <WidgetTitle className="flex-1" icon={RouteRounded}>
            {m.dashboard_widget_proxy_mode_title()}
          </WidgetTitle>

          {actionState === 'unconfirmed' && (
            <>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="basic"
                    icon
                    className="ml-auto size-7 shrink-0"
                    aria-label={m.dashboard_widget_operation_check()}
                    disabled={disabled || checkingStatus}
                    loading={checkingStatus}
                    onClick={checkStatus}
                  >
                    <RefreshRounded className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {m.dashboard_widget_operation_check()}
                </TooltipContent>
              </Tooltip>
            </>
          )}
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
        </WidgetHeader>

        <CardContent className="min-h-0 flex-1 justify-start gap-1 overflow-hidden px-4 py-3">
          <ProxyModeOptions
            modes={supportedModes}
            currentMode={currentMode}
            layout={layout}
            disabled={!actionable || proxyMode.isPending}
            onChange={changeMode}
          />

          {actionState !== 'idle' && (
            <p
              className={
                actionState === 'failed'
                  ? 'text-error text-xs'
                  : 'text-on-surface-variant text-xs'
              }
              role={actionState === 'failed' ? 'alert' : 'status'}
            >
              {checkFailed
                ? m.dashboard_widget_operation_check_failed()
                : actionState === 'degraded'
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

  if (sourceOnly || isOverlay) return <ProxyModePreview id={props.id} />

  return <ProxyModeLive {...props} disabled={!disabled} />
}
