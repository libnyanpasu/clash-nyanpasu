import NorthEastRounded from '~icons/material-symbols/north-east-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import RefreshRounded from '~icons/material-symbols/refresh-rounded'
import RouteRounded from '~icons/material-symbols/route-rounded'
import { useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { ExpressiveChoice } from '@nyanpasu/ui/expressive-choice'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { useCapabilities } from '@/components/auth/capabilities'
import { m } from '@/paraglide/messages'
import {
  MutationUnconfirmedError,
  useCoreStatus,
  useProxyMode,
  type ProxyMode,
} from '@nyanpasu/query'
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  useWidgetInteractive,
  Widget,
  WidgetContent,
  WidgetControls,
  WidgetHeader,
  WidgetTitle,
} from './core'
import { DEFAULT_WIDGET_CONFIGS, WidgetId } from './widget-config'
import { ConfigChoice } from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'

const MODE_KEYS: ProxyMode[] = ['rule', 'global', 'direct']
const MODE_ICONS = {
  rule: RouteRounded,
  global: PublicRounded,
  direct: NorthEastRounded,
}

function modeLabel(mode: ProxyMode): string {
  switch (mode) {
    case 'rule':
      return m.dashboard_widget_proxy_mode_rule()
    case 'global':
      return m.dashboard_widget_proxy_mode_global()
    case 'direct':
      return m.dashboard_widget_proxy_mode_direct()
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

function ProxyModePreview() {
  const config = useWidgetConfig(WidgetId.ProxyMode)

  return (
    <Widget scaling="proportional" data-widget-kind="proxy-mode">
      <WidgetHeader>
        <WidgetTitle />
      </WidgetHeader>
      <WidgetContent className="justify-start gap-1">
        <ProxyModeOptions
          modes={MODE_KEYS}
          currentMode="rule"
          disabled
          layout={config.layout}
        />
      </WidgetContent>
    </Widget>
  )
}

type ActionState = 'idle' | 'degraded' | 'unconfirmed' | 'failed'

function ProxyModeLive() {
  const proxyMode = useProxyMode()
  const config = useWidgetConfig(WidgetId.ProxyMode)
  const coreStatus = useCoreStatus()
  const interactive = useWidgetInteractive()
  const canProxy = useCapabilities().hasPermission('proxy:write')
  const [actionState, setActionState] = useState<ActionState>('idle')
  const [checkedAfterUnconfirmed, setCheckedAfterUnconfirmed] = useState(false)
  const [checkingStatus, setCheckingStatus] = useState(false)
  const [checkFailed, setCheckFailed] = useState(false)
  const configData = proxyMode.query.data
  const layout = config.layout
  const rawMode = configData?.mode?.toLowerCase()
  const currentMode = MODE_KEYS.find((mode) => mode === rawMode) ?? null
  const waiting = proxyMode.query.isPending || coreStatus.isPending
  const isOffline =
    coreStatus.data !== undefined &&
    coreStatus.data !== null &&
    coreStatus.data.status !== 'Running'
  const loadFailed = (proxyMode.query.isError || coreStatus.isError) && !waiting
  const actionable =
    interactive &&
    canProxy &&
    (actionState !== 'unconfirmed' || checkedAfterUnconfirmed) &&
    !checkingStatus &&
    !waiting &&
    !loadFailed &&
    !isOffline &&
    configData !== undefined &&
    coreStatus.data?.status === 'Running'

  const changeMode = async (mode: ProxyMode) => {
    if (!actionable || proxyMode.isPending || mode === currentMode) return
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
    if (!interactive || checkingStatus) return
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
    <Widget
      scaling="proportional"
      data-widget-kind="proxy-mode"
      data-slot="proxy-mode-card"
    >
      <WidgetHeader className="flex-wrap gap-x-2 gap-y-1">
        <WidgetTitle />

        <WidgetControls className="ml-auto">
          {actionState === 'unconfirmed' && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="basic"
                  icon
                  className="size-7 shrink-0"
                  aria-label={m.dashboard_widget_operation_check()}
                  disabled={!interactive || checkingStatus}
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
        </WidgetControls>
      </WidgetHeader>

      <WidgetContent className="justify-start gap-1">
        <ProxyModeOptions
          modes={MODE_KEYS}
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
      </WidgetContent>
    </Widget>
  )
}

function ProxyModeConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions(
    WidgetId.ProxyMode,
  )

  return (
    <ConfigGroup>
      <ConfigChoice
        label={m.dashboard_widget_proxy_mode_config_layout()}
        value={config.layout}
        disabled={disabled}
        options={[
          {
            value: 'focus',
            label: m.dashboard_widget_proxy_mode_config_focus(),
          },
          {
            value: 'flex',
            label: m.dashboard_widget_proxy_mode_config_flex(),
          },
        ]}
        onChange={(layout) => updateConfig({ layout })}
      />
    </ConfigGroup>
  )
}

export const proxyModeWidget = defineWidget({
  id: WidgetId.ProxyMode,
  category: 'controls',
  title: m.dashboard_widget_proxy_mode_title,
  icon: RouteRounded,
  size: { minW: 4, minH: 2, defaultW: 4, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.ProxyMode],
  Component: ProxyModeLive,
  Preview: ProxyModePreview,
  ConfigForm: ProxyModeConfigForm,
})
