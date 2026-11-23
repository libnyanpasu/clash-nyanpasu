import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import type { DashboardItem, WidgetComponentProps } from './consts'
import { WidgetGridHost, WidgetOverlayHost, WidgetPreview } from './core'
import { useDashboardContext } from './provider'
import { getWidgetDefinition } from './registry'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WidgetConfig,
  WidgetId,
} from './widget-config'
import { WidgetConfigControl } from './widget-config-control'

/**
 * Browser fixtures that still mount one widget directly. They render through
 * the same core hosts as the dashboard, so source and overlay rendering stay
 * identical to the production path.
 */
function WidgetRenderer({
  type,
  id,
  onCloseClick,
}: WidgetComponentProps & { type: WidgetId }) {
  const { sourceOnly, isOverlay, disabled, displayItems } = useDndGridContext()
  const { configs, saveConfig, setIsEditing, configLoading, configReadError } =
    useDashboardContext()
  const definition = getWidgetDefinition(type)
  const config = getWidgetConfig(configs, id, type)

  if (sourceOnly) {
    return <WidgetPreview definition={definition} />
  }

  if (isOverlay) {
    return (
      <WidgetOverlayHost type={type} definition={definition} config={config} />
    )
  }

  const found = displayItems.find((entry) => entry.id === id)
  const item: DashboardItem = found
    ? { ...found, type: (found as Partial<DashboardItem>).type ?? type }
    : {
        id,
        type,
        x: 0,
        y: 0,
        w: definition.size.defaultW,
        h: definition.size.defaultH,
      }

  return (
    <WidgetGridHost
      item={item}
      definition={definition}
      config={config}
      configDisabled={configLoading || configReadError}
      interactive={disabled}
      isEditing={!disabled}
      updateConfig={(patch) =>
        saveConfig(id, { ...config, ...patch } as WidgetConfig)
      }
      resetConfig={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
      enterEditMode={() => setIsEditing(true)}
      onRemove={onCloseClick}
    >
      <WidgetConfigControl id={id} type={type} isActive={!disabled} />
    </WidgetGridHost>
  )
}

export function LegacyWidgetRenderer({
  type,
  ...props
}: WidgetComponentProps & { type: WidgetId }) {
  return <WidgetRenderer type={type} {...props} />
}

function createLegacyWidget(type: WidgetId) {
  function LegacyWidget(props: WidgetComponentProps) {
    return <WidgetRenderer type={type} {...props} />
  }

  LegacyWidget.displayName = `LegacyWidget(${type})`
  return LegacyWidget
}

export const TrafficDownWidget = createLegacyWidget(WidgetId.TrafficDown)
export const TrafficUpWidget = createLegacyWidget(WidgetId.TrafficUp)
export const ConnectionsWidget = createLegacyWidget(WidgetId.Connections)
export const MemoryWidget = createLegacyWidget(WidgetId.Memory)
export const ProxyShortcutsWidget = createLegacyWidget(WidgetId.ProxyShortcuts)
export const CoreShortcutsWidget = createLegacyWidget(WidgetId.CoreShortcuts)
export const SubscriptionQuotaWidget = createLegacyWidget(
  WidgetId.SubscriptionQuota,
)
export const SubscriptionScheduleWidget = createLegacyWidget(
  WidgetId.SubscriptionSchedule,
)
export const ProxyModeWidget = createLegacyWidget(WidgetId.ProxyMode)
export const RecentTrafficWidget = createLegacyWidget(WidgetId.RecentTraffic)
export const OriginTrafficWidget = createLegacyWidget(WidgetId.OriginTraffic)
export const ExitTrafficWidget = createLegacyWidget(WidgetId.ExitTraffic)
export const TargetTrafficWidget = createLegacyWidget(WidgetId.TargetTraffic)
export const RuleTrafficWidget = createLegacyWidget(WidgetId.RuleTraffic)
export const ActiveConnectionsWidget = createLegacyWidget(
  WidgetId.ActiveConnections,
)
export const ProviderUpdatesWidget = createLegacyWidget(
  WidgetId.ProviderUpdates,
)
export const NetworkInfoWidget = createLegacyWidget(WidgetId.NetworkInfo)
