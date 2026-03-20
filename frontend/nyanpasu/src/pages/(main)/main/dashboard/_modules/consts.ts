import type { ResponsiveLayouts } from 'react-grid-layout'
import { WidgetDef } from './types'
import { CoreShortcutsWidget, ProxyShortcutsWidget } from './widget-shortcut'
import {
  ConnectionsWidget,
  MemoryWidget,
  TrafficDownWidget,
  TrafficUpWidget,
} from './widget-sparkline'

export const WIDGETS = [
  {
    id: 'traffic-down',
    title: 'Download Traffic',
    Component: TrafficDownWidget,
    minW: 2,
    minH: 2,
  },
  {
    id: 'traffic-up',
    title: 'Upload Traffic',
    Component: TrafficUpWidget,
    minW: 2,
    minH: 2,
  },
  {
    id: 'connections',
    title: 'Active Connections',
    Component: ConnectionsWidget,
    minW: 2,
    minH: 2,
  },
  {
    id: 'memory',
    title: 'Memory',
    Component: MemoryWidget,
    minW: 2,
    minH: 2,
  },
  {
    id: 'proxy-shortcuts',
    title: 'Proxy Shortcuts',
    Component: ProxyShortcutsWidget,
    minW: 3,
    minH: 2,
  },
  {
    id: 'core-shortcuts',
    title: 'Core Shortcuts',
    Component: CoreShortcutsWidget,
    minW: 3,
    minH: 2,
  },
] as const satisfies readonly WidgetDef[]

export type WidgetId = (typeof WIDGETS)[number]['id']

export const defineWidget = (
  id: WidgetId,
  rect: {
    x: number
    y: number
    w: number
    h: number
  },
  min: {
    w: number
    h: number
  } = {
    w: 2,
    h: 2,
  },
) => ({
  i: id,
  ...rect,
  minW: min.w,
  minH: min.h,
})

export const DEFAULT_LAYOUTS = {
  lg: [
    defineWidget('traffic-down', {
      x: 0,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget('traffic-up', {
      x: 3,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget('connections', {
      x: 6,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget('memory', {
      x: 9,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget(
      'proxy-shortcuts',
      {
        x: 0,
        y: 3,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
    defineWidget(
      'core-shortcuts',
      {
        x: 4,
        y: 3,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
  ],
  md: [
    defineWidget('traffic-down', {
      x: 0,
      y: 0,
      w: 2,
      h: 3,
    }),
    defineWidget('traffic-up', {
      x: 2,
      y: 0,
      w: 2,
      h: 3,
    }),
    defineWidget('connections', {
      x: 4,
      y: 0,
      w: 2,
      h: 3,
    }),
    defineWidget('memory', {
      x: 6,
      y: 0,
      w: 2,
      h: 3,
    }),
    defineWidget(
      'proxy-shortcuts',
      {
        x: 0,
        y: 3,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
    defineWidget(
      'core-shortcuts',
      {
        x: 4,
        y: 3,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
  ],
  sm: [
    defineWidget('traffic-down', {
      x: 0,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget('traffic-up', {
      x: 3,
      y: 0,
      w: 3,
      h: 3,
    }),
    defineWidget('connections', {
      x: 0,
      y: 3,
      w: 3,
      h: 3,
    }),
    defineWidget('memory', {
      x: 3,
      y: 3,
      w: 3,
      h: 3,
    }),
    defineWidget(
      'proxy-shortcuts',
      {
        x: 0,
        y: 6,
        w: 3,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
    defineWidget(
      'core-shortcuts',
      {
        x: 3,
        y: 6,
        w: 3,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
  ],
  xs: [
    defineWidget('traffic-down', {
      x: 0,
      y: 0,
      w: 2,
      h: 2,
    }),
    defineWidget('traffic-up', {
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    }),
    defineWidget('connections', {
      x: 0,
      y: 2,
      w: 2,
      h: 2,
    }),
    defineWidget('memory', {
      x: 2,
      y: 2,
      w: 2,
      h: 2,
    }),
    defineWidget(
      'proxy-shortcuts',
      {
        x: 0,
        y: 4,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
    defineWidget(
      'core-shortcuts',
      {
        x: 0,
        y: 7,
        w: 4,
        h: 3,
      },
      {
        w: 3,
        h: 2,
      },
    ),
  ],
} satisfies ResponsiveLayouts
