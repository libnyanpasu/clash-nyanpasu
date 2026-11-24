import type { WidgetDefinition } from './core/define-widget'
import { registerWidgets } from './core/registry'
import { activeConnectionsWidget } from './widget-active-connections'
import { networkInfoWidget } from './widget-network-info'
import { providerUpdatesWidget } from './widget-provider-updates'
import { proxyModeWidget } from './widget-proxy-mode'
import { coreShortcutsWidget, proxyShortcutsWidget } from './widget-shortcut'
import {
  connectionsWidget,
  memoryWidget,
  trafficDownWidget,
  trafficUpWidget,
} from './widget-sparkline'
import { subscriptionQuotaWidget } from './widget-subscription-quota'
import { subscriptionScheduleWidget } from './widget-subscription-schedule'
import {
  exitTrafficWidget,
  originTrafficWidget,
  recentTrafficWidget,
  ruleTrafficWidget,
  targetTrafficWidget,
} from './widget-traffic-report'

/** The catalog order is the order the library and layouts have always used. */
export const WIDGET_LIST: readonly WidgetDefinition[] = [
  trafficDownWidget,
  trafficUpWidget,
  connectionsWidget,
  memoryWidget,
  proxyShortcutsWidget,
  coreShortcutsWidget,
  subscriptionQuotaWidget,
  subscriptionScheduleWidget,
  proxyModeWidget,
  recentTrafficWidget,
  originTrafficWidget,
  exitTrafficWidget,
  targetTrafficWidget,
  ruleTrafficWidget,
  activeConnectionsWidget,
  providerUpdatesWidget,
  networkInfoWidget,
]

registerWidgets(WIDGET_LIST)

export {
  getWidgetDefinition,
  getWidgetDefinitions,
  WIDGET_CATEGORY_IDS,
  WIDGET_REGISTRY,
} from './core/registry'
