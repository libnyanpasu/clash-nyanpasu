import type { DndGridItemType } from '@nyanpasu/ui/dnd-grid'
import { WidgetId } from './widget-config'

export { WidgetId } from './widget-config'
export {
  RENDER_MAP,
  WIDGET_MIN_SIZE_MAP,
  WIDGET_RECOMMENDED_SIZE_MAP,
} from './widget-renderer'

export type DashboardItem = DndGridItemType<string> & { type: WidgetId }

export type WidgetComponentProps = {
  id: string
  onCloseClick?: (id: string) => void
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
