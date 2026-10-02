import type { TrafficRange } from '@nyanpasu/rpc/types'

export enum WidgetId {
  TrafficDown = 'traffic-down',
  TrafficUp = 'traffic-up',
  Connections = 'connections',
  Memory = 'memory',
  ProxyShortcuts = 'proxy-shortcuts',
  CoreShortcuts = 'core-shortcuts',
  SubscriptionQuota = 'subscription-quota',
  SubscriptionSchedule = 'subscription-schedule',
  ProfileShortcuts = 'profile-shortcuts',
  ProxyMode = 'proxy-mode',
  RecentTraffic = 'recent-traffic',
  OriginTraffic = 'origin-traffic',
  ExitTraffic = 'exit-traffic',
  TargetTraffic = 'target-traffic',
  RuleTraffic = 'rule-traffic',
  ActiveConnections = 'active-connections',
  ConfigurationHealth = 'configuration-health',
  ProviderUpdates = 'provider-updates',
}

export const PROXY_HORIZONTAL_MIN_WIDTH = 6

type TrafficConfig = {
  showChart: boolean
  showTotal: boolean
  unit: 'bytes' | 'bits'
}

type HistoryConfig = { showChart: boolean; samples: 8 | 16 | 32 }

export type SubscriptionTarget =
  { kind: 'current' } | { kind: 'fixed'; profileUid: string }

export type ReportWidgetConfig = {
  range: TrafficRange
  profileUid: string | null
  showDirections: boolean
}

type RankingConfig = ReportWidgetConfig & {
  topN: 3 | 5
  hideNames: boolean
}

export type ProviderReference = { kind: 'proxy' | 'rule'; name: string }

export type WidgetConfigs = {
  [WidgetId.SubscriptionQuota]: {
    type: WidgetId.SubscriptionQuota
    target: SubscriptionTarget
    showExpiry: boolean
    showProgress: boolean
    expiryWarningDays: 7 | 14 | 30
    quotaWarningPercent: 10 | 20 | 30
  }
  [WidgetId.SubscriptionSchedule]: {
    type: WidgetId.SubscriptionSchedule
    target: SubscriptionTarget
    showRecentRuns: boolean
  }
  [WidgetId.ProfileShortcuts]: {
    type: WidgetId.ProfileShortcuts
    profileUids: string[]
  }
  [WidgetId.ProxyMode]: { type: WidgetId.ProxyMode; showHelp: boolean }
  [WidgetId.RecentTraffic]: ReportWidgetConfig & {
    type: WidgetId.RecentTraffic
  }
  [WidgetId.OriginTraffic]: RankingConfig & { type: WidgetId.OriginTraffic }
  [WidgetId.ExitTraffic]: RankingConfig & { type: WidgetId.ExitTraffic }
  [WidgetId.TargetTraffic]: RankingConfig & { type: WidgetId.TargetTraffic }
  [WidgetId.RuleTraffic]: RankingConfig & { type: WidgetId.RuleTraffic }
  [WidgetId.ActiveConnections]: {
    type: WidgetId.ActiveConnections
    sort: 'download' | 'upload' | 'total'
    topN: 3 | 5
    showProcess: boolean
    hideTargets: boolean
  }
  [WidgetId.ConfigurationHealth]: {
    type: WidgetId.ConfigurationHealth
    showSources: boolean
    maxItems: 1 | 3 | 5
  }
  [WidgetId.ProviderUpdates]: {
    type: WidgetId.ProviderUpdates
    kinds: 'both' | 'proxy' | 'rule'
    resources: ProviderReference[]
    maxItems: 3 | 5
  }
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

const reportDefaults: ReportWidgetConfig = {
  range: 'last24_hours',
  profileUid: null,
  showDirections: true,
}
const rankingDefaults: RankingConfig = {
  ...reportDefaults,
  topN: 3,
  hideNames: false,
}

export const DEFAULT_WIDGET_CONFIGS: WidgetConfigs = {
  [WidgetId.SubscriptionQuota]: {
    type: WidgetId.SubscriptionQuota,
    target: { kind: 'current' },
    showExpiry: true,
    showProgress: true,
    expiryWarningDays: 7,
    quotaWarningPercent: 20,
  },
  [WidgetId.SubscriptionSchedule]: {
    type: WidgetId.SubscriptionSchedule,
    target: { kind: 'current' },
    showRecentRuns: true,
  },
  [WidgetId.ProfileShortcuts]: {
    type: WidgetId.ProfileShortcuts,
    profileUids: [],
  },
  [WidgetId.ProxyMode]: { type: WidgetId.ProxyMode, showHelp: true },
  [WidgetId.RecentTraffic]: { ...reportDefaults, type: WidgetId.RecentTraffic },
  [WidgetId.OriginTraffic]: {
    ...rankingDefaults,
    type: WidgetId.OriginTraffic,
  },
  [WidgetId.ExitTraffic]: { ...rankingDefaults, type: WidgetId.ExitTraffic },
  [WidgetId.TargetTraffic]: {
    ...rankingDefaults,
    type: WidgetId.TargetTraffic,
  },
  [WidgetId.RuleTraffic]: { ...rankingDefaults, type: WidgetId.RuleTraffic },
  [WidgetId.ActiveConnections]: {
    type: WidgetId.ActiveConnections,
    sort: 'download',
    topN: 3,
    showProcess: true,
    hideTargets: false,
  },
  [WidgetId.ConfigurationHealth]: {
    type: WidgetId.ConfigurationHealth,
    showSources: true,
    maxItems: 3,
  },
  [WidgetId.ProviderUpdates]: {
    type: WidgetId.ProviderUpdates,
    kinds: 'both',
    resources: [],
    maxItems: 3,
  },
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
      } else if (key === 'target' && isRecord(candidate)) {
        if (candidate.kind === 'current')
          Object.assign(config, { target: { kind: 'current' } })
        else if (
          candidate.kind === 'fixed' &&
          isReference(candidate.profileUid)
        ) {
          Object.assign(config, {
            target: { kind: 'fixed', profileUid: candidate.profileUid },
          })
        }
      } else if (
        key === 'profileUid' &&
        (candidate === null || isReference(candidate))
      ) {
        Object.assign(config, { profileUid: candidate })
      } else if (
        key === 'profileUids' &&
        Array.isArray(candidate) &&
        candidate.every(isReference)
      ) {
        Object.assign(config, { profileUids: [...new Set(candidate)] })
      } else if (
        key === 'resources' &&
        Array.isArray(candidate) &&
        candidate.every(
          (entry) =>
            isRecord(entry) &&
            (entry.kind === 'proxy' || entry.kind === 'rule') &&
            isReference(entry.name),
        )
      ) {
        const unique = new Map(
          candidate.map((entry) => [
            JSON.stringify([entry.kind, entry.name]),
            { kind: entry.kind, name: entry.name },
          ]),
        )
        Object.assign(config, { resources: [...unique.values()] })
      } else if (
        (key === 'range' &&
          typeof candidate === 'string' &&
          [
            'last_hour',
            'last6_hours',
            'last24_hours',
            'last7_days',
            'last30_days',
            'all',
          ].includes(candidate)) ||
        (key === 'topN' && (candidate === 3 || candidate === 5)) ||
        (key === 'maxItems' &&
          (candidate === 3 ||
            candidate === 5 ||
            (config.type === WidgetId.ConfigurationHealth &&
              candidate === 1))) ||
        (key === 'sort' &&
          typeof candidate === 'string' &&
          ['download', 'upload', 'total'].includes(candidate)) ||
        (key === 'kinds' &&
          typeof candidate === 'string' &&
          ['both', 'proxy', 'rule'].includes(candidate)) ||
        (key === 'expiryWarningDays' &&
          [7, 14, 30].includes(candidate as number)) ||
        (key === 'quotaWarningPercent' &&
          [10, 20, 30].includes(candidate as number)) ||
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

const isReference = (value: unknown): value is string =>
  typeof value === 'string' && value.trim().length > 0

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
