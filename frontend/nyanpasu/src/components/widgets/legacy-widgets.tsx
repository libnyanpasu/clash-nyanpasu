import { RENDER_MAP, type WidgetComponentProps } from './consts'
import { WidgetId } from './widget-config'

/**
 * Named adapters for browser fixtures that still mount one widget directly.
 * They route through the same RENDER_MAP bridge as the dashboard, so source
 * and overlay rendering stay identical to the production path.
 */
function createLegacyWidget(type: WidgetId) {
  function LegacyWidget(props: WidgetComponentProps) {
    const Widget = RENDER_MAP[type]
    return <Widget {...props} />
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
