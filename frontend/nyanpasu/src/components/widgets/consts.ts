import { memo, ReactNode } from 'react'
import type { DndGridItemType } from '@nyanpasu/ui/dnd-grid'
import { ActiveConnectionsWidget } from './widget-active-connections'
import { WidgetId } from './widget-config'
import { ProviderUpdatesWidget } from './widget-provider-updates'
import { ProxyModeWidget } from './widget-proxy-mode'
import { CoreShortcutsWidget, ProxyShortcutsWidget } from './widget-shortcut'
import {
  ConnectionsWidget,
  MemoryWidget,
  TrafficDownWidget,
  TrafficUpWidget,
} from './widget-sparkline'
import { SubscriptionQuotaWidget } from './widget-subscription-quota'
import { SubscriptionScheduleWidget } from './widget-subscription-schedule'
import {
  ExitTrafficWidget,
  OriginTrafficWidget,
  RecentTrafficWidget,
  RuleTrafficWidget,
  TargetTrafficWidget,
} from './widget-traffic-report'

export { WidgetId } from './widget-config'

export type DashboardItem = DndGridItemType<string> & { type: WidgetId }

export type WidgetComponentProps = {
  id: string
  onCloseClick?: (id: string) => void
}

// Memoized: the grid renders every widget whenever the dashboard re-renders.
export const RENDER_MAP: Record<
  WidgetId,
  (props: WidgetComponentProps) => ReactNode
> = {
  [WidgetId.TrafficDown]: memo(TrafficDownWidget),
  [WidgetId.TrafficUp]: memo(TrafficUpWidget),
  [WidgetId.Connections]: memo(ConnectionsWidget),
  [WidgetId.Memory]: memo(MemoryWidget),
  [WidgetId.ProxyShortcuts]: memo(ProxyShortcutsWidget),
  [WidgetId.CoreShortcuts]: memo(CoreShortcutsWidget),
  [WidgetId.SubscriptionQuota]: memo(SubscriptionQuotaWidget),
  [WidgetId.SubscriptionSchedule]: memo(SubscriptionScheduleWidget),
  [WidgetId.ProxyMode]: memo(ProxyModeWidget),
  [WidgetId.RecentTraffic]: memo(RecentTrafficWidget),
  [WidgetId.OriginTraffic]: memo(OriginTrafficWidget),
  [WidgetId.ExitTraffic]: memo(ExitTrafficWidget),
  [WidgetId.TargetTraffic]: memo(TargetTrafficWidget),
  [WidgetId.RuleTraffic]: memo(RuleTrafficWidget),
  [WidgetId.ActiveConnections]: memo(ActiveConnectionsWidget),
  [WidgetId.ProviderUpdates]: memo(ProviderUpdatesWidget),
}

/** Default layout, designed for a 12-column grid. */
export const DEFAULT_ITEMS: DashboardItem[] = [
  {
    id: WidgetId.TrafficDown,
    type: WidgetId.TrafficDown,
    x: 0,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.TrafficUp,
    type: WidgetId.TrafficUp,
    x: 3,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.Memory,
    type: WidgetId.Memory,
    x: 6,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.Connections,
    type: WidgetId.Connections,
    x: 9,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.ProxyShortcuts,
    type: WidgetId.ProxyShortcuts,
    x: 0,
    y: 2,
    w: 3,
    h: 3,
  },
  {
    id: WidgetId.CoreShortcuts,
    type: WidgetId.CoreShortcuts,
    x: 3,
    y: 2,
    w: 4,
    h: 2,
  },
]

export const WIDGET_MIN_SIZE_MAP: Record<
  WidgetId,
  { minW: number; minH: number }
> = {
  [WidgetId.TrafficDown]: { minW: 2, minH: 2 },
  [WidgetId.TrafficUp]: { minW: 2, minH: 2 },
  [WidgetId.Connections]: { minW: 2, minH: 2 },
  [WidgetId.Memory]: { minW: 2, minH: 2 },
  [WidgetId.ProxyShortcuts]: { minW: 3, minH: 2 },
  [WidgetId.CoreShortcuts]: { minW: 4, minH: 2 },
  [WidgetId.SubscriptionQuota]: { minW: 3, minH: 2 },
  [WidgetId.SubscriptionSchedule]: { minW: 3, minH: 2 },
  [WidgetId.ProxyMode]: { minW: 4, minH: 2 },
  [WidgetId.RecentTraffic]: { minW: 3, minH: 2 },
  [WidgetId.OriginTraffic]: { minW: 3, minH: 2 },
  [WidgetId.ExitTraffic]: { minW: 3, minH: 2 },
  [WidgetId.TargetTraffic]: { minW: 3, minH: 2 },
  [WidgetId.RuleTraffic]: { minW: 3, minH: 2 },
  [WidgetId.ActiveConnections]: { minW: 4, minH: 2 },
  [WidgetId.ProviderUpdates]: { minW: 3, minH: 2 },
}

export const WIDGET_RECOMMENDED_SIZE_MAP: Partial<
  Record<WidgetId, { w: number; h: number }>
> = {
  [WidgetId.SubscriptionQuota]: { w: 4, h: 3 },
  [WidgetId.SubscriptionSchedule]: { w: 4, h: 3 },
  [WidgetId.ProxyMode]: { w: 4, h: 2 },
  [WidgetId.RecentTraffic]: { w: 4, h: 2 },
  [WidgetId.OriginTraffic]: { w: 4, h: 4 },
  [WidgetId.ExitTraffic]: { w: 4, h: 4 },
  [WidgetId.TargetTraffic]: { w: 4, h: 4 },
  [WidgetId.RuleTraffic]: { w: 4, h: 4 },
  [WidgetId.ActiveConnections]: { w: 6, h: 3 },
  [WidgetId.ProviderUpdates]: { w: 4, h: 3 },
}

export type LayoutStorage = Record<string, DashboardItem[]>

// preset layouts for common grid sizes
export const DEFAULT_LAYOUTS: LayoutStorage = {
  '4x5': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 0,
      y: 2,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 2,
      y: 2,
      w: 2,
      h: 2,
    },
  ],
  '8x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 4,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 6,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 3,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  '12x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 3,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 6,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 9,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 3,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  '16x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 4,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 8,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 12,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 4,
      h: 3,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 4,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  '20x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 5,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 10,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 15,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 5,
      h: 3,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 5,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
}
