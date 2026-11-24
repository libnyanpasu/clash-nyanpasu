import AnalyticsRounded from '~icons/material-symbols/analytics-rounded'
import AppsRounded from '~icons/material-symbols/apps-rounded'
import MonitoringRounded from '~icons/material-symbols/monitoring-rounded'
import PublicRounded from '~icons/material-symbols/public-rounded'
import SubscriptionsRounded from '~icons/material-symbols/subscriptions-rounded'
import TuneRounded from '~icons/material-symbols/tune-rounded'
import type { ComponentType } from 'react'
import { m } from '@/paraglide/messages'
import type { WidgetCategoryId, WidgetDefinition } from './core/define-widget'
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

export type WidgetCategoryFilter = 'all' | WidgetCategoryId

export type WidgetCategoryDefinition = {
  id: WidgetCategoryFilter
  label: () => string
  icon: ComponentType<{ className?: string }>
}

/** The gallery category order; `all` lists the complete catalog. */
export const WIDGET_CATEGORIES: readonly WidgetCategoryDefinition[] = [
  {
    id: 'all',
    label: m.dashboard_widget_category_all,
    icon: AppsRounded,
  },
  {
    id: 'realtime',
    label: m.dashboard_widget_category_realtime,
    icon: MonitoringRounded,
  },
  {
    id: 'analytics',
    label: m.dashboard_widget_category_analytics,
    icon: AnalyticsRounded,
  },
  {
    id: 'controls',
    label: m.dashboard_widget_category_controls,
    icon: TuneRounded,
  },
  {
    id: 'subscription',
    label: m.dashboard_widget_category_subscription,
    icon: SubscriptionsRounded,
  },
  {
    id: 'network',
    label: m.dashboard_widget_category_network,
    icon: PublicRounded,
  },
]

export {
  getWidgetDefinition,
  getWidgetDefinitions,
  WIDGET_CATEGORY_IDS,
  WIDGET_REGISTRY,
} from './core/registry'
