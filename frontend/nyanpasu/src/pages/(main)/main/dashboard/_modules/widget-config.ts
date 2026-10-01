export enum WidgetId {
  TrafficDown = 'traffic-down',
  TrafficUp = 'traffic-up',
  Connections = 'connections',
  Memory = 'memory',
  ProxyShortcuts = 'proxy-shortcuts',
  CoreShortcuts = 'core-shortcuts',
}

export const PROXY_HORIZONTAL_MIN_WIDTH = 6

type TrafficConfig = {
  showChart: boolean
  showTotal: boolean
  unit: 'bytes' | 'bits'
}

type HistoryConfig = { showChart: boolean; samples: 8 | 16 | 32 }

export type WidgetConfigs = {
  [WidgetId.TrafficDown]: TrafficConfig & { type: WidgetId.TrafficDown }
  [WidgetId.TrafficUp]: TrafficConfig & { type: WidgetId.TrafficUp }
  [WidgetId.Memory]: HistoryConfig & { type: WidgetId.Memory }
  [WidgetId.Connections]: HistoryConfig & { type: WidgetId.Connections }
  [WidgetId.ProxyShortcuts]: {
    type: WidgetId.ProxyShortcuts
    orientation: 'vertical' | 'horizontal'
    buttons: 'both' | 'system' | 'tun'
    order: 'system-first' | 'tun-first'
  }
  [WidgetId.CoreShortcuts]: {
    type: WidgetId.CoreShortcuts
    density: 'detailed' | 'compact'
    showVersion: boolean
    showChannel: boolean
  }
}

export type WidgetConfig = WidgetConfigs[WidgetId]

export const DEFAULT_WIDGET_CONFIGS: WidgetConfigs = {
  [WidgetId.TrafficDown]: {
    type: WidgetId.TrafficDown,
    showChart: true,
    showTotal: true,
    unit: 'bytes',
  },
  [WidgetId.TrafficUp]: {
    type: WidgetId.TrafficUp,
    showChart: true,
    showTotal: true,
    unit: 'bytes',
  },
  [WidgetId.Memory]: { type: WidgetId.Memory, showChart: true, samples: 32 },
  [WidgetId.Connections]: {
    type: WidgetId.Connections,
    showChart: true,
    samples: 32,
  },
  [WidgetId.ProxyShortcuts]: {
    type: WidgetId.ProxyShortcuts,
    orientation: 'vertical',
    buttons: 'both',
    order: 'system-first',
  },
  [WidgetId.CoreShortcuts]: {
    type: WidgetId.CoreShortcuts,
    density: 'detailed',
    showVersion: true,
    showChannel: true,
  },
}

export type WidgetConfigStorage = {
  version: 1
  byInstance: Record<string, WidgetConfig>
}

export const EMPTY_WIDGET_CONFIG_STORAGE: WidgetConfigStorage = {
  version: 1,
  byInstance: {},
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

/** Validate cached and persisted options without changing the layout schema. */
export function normalizeWidgetConfigStorage(
  raw: unknown,
): WidgetConfigStorage {
  if (!isRecord(raw) || raw.version !== 1 || !isRecord(raw.byInstance)) {
    return EMPTY_WIDGET_CONFIG_STORAGE
  }

  const byInstance: Record<string, WidgetConfig> = {}
  for (const [id, value] of Object.entries(raw.byInstance)) {
    if (!isRecord(value)) continue
    const defaults = Object.values(DEFAULT_WIDGET_CONFIGS).find(
      (config) => config.type === value.type,
    )
    if (!defaults) continue
    // Only accept known keys and values. Missing fields keep their defaults.
    const config = { ...defaults }
    for (const [key, defaultValue] of Object.entries(defaults)) {
      const candidate = value[key]
      if (typeof candidate === 'boolean' && typeof defaultValue === 'boolean') {
        Object.assign(config, { [key]: candidate })
      } else if (
        (key === 'samples' &&
          typeof candidate === 'number' &&
          [8, 16, 32].includes(candidate)) ||
        (key === 'unit' && (candidate === 'bytes' || candidate === 'bits')) ||
        (key === 'orientation' &&
          (candidate === 'vertical' || candidate === 'horizontal')) ||
        (key === 'buttons' &&
          (candidate === 'both' ||
            candidate === 'system' ||
            candidate === 'tun')) ||
        (key === 'order' &&
          (candidate === 'system-first' || candidate === 'tun-first')) ||
        (key === 'density' &&
          (candidate === 'detailed' || candidate === 'compact'))
      ) {
        Object.assign(config, { [key]: candidate })
      }
    }
    Object.defineProperty(byInstance, id, {
      value: config,
      enumerable: true,
      configurable: true,
      writable: true,
    })
  }
  return { version: 1, byInstance }
}

export function getWidgetConfig<T extends WidgetId>(
  storage: WidgetConfigStorage,
  id: string,
  type: T,
): WidgetConfigs[T] {
  const saved = storage.byInstance[id]
  return (
    saved?.type === type ? saved : DEFAULT_WIDGET_CONFIGS[type]
  ) as WidgetConfigs[T]
}
