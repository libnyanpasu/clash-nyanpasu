import { expect, test } from 'vitest'
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  getWidgetConfig,
  normalizeWidgetConfigStorage,
  WidgetId,
} from '@/components/widgets/widget-config'

test('missing, unsupported and malformed storage keep existing widget defaults', () => {
  for (const value of [null, [], {}, { version: 2, byInstance: {} }]) {
    expect(normalizeWidgetConfigStorage(value)).toEqual(
      EMPTY_WIDGET_CONFIG_STORAGE,
    )
  }
  expect(
    getWidgetConfig(
      EMPTY_WIDGET_CONFIG_STORAGE,
      'old-layout-id',
      WidgetId.ProxyShortcuts,
    ),
  ).toEqual(DEFAULT_WIDGET_CONFIGS[WidgetId.ProxyShortcuts])
})

test('cached options accept known values and default invalid or missing fields', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      download: {
        type: 'traffic-down',
        showChart: false,
        showTotal: 'false',
        unit: 'unknown',
        extra: true,
      },
      memory: { type: 'memory', samples: 16 },
      invalidSamples: { type: 'connections', samples: '8' },
      unknown: { type: 'unknown', showChart: false },
    },
  })
  expect(storage.byInstance.download).toEqual({
    type: WidgetId.TrafficDown,
    showChart: false,
    showTotal: true,
    unit: 'bytes',
  })
  expect(storage.byInstance.memory).toEqual({
    type: WidgetId.Memory,
    showChart: true,
    samples: 16,
  })
  expect(storage.byInstance.invalidSamples).toEqual(
    DEFAULT_WIDGET_CONFIGS[WidgetId.Connections],
  )
  expect(storage.byInstance.unknown).toBeUndefined()
})

test('duplicate instances are independent and a mismatched type uses defaults', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      first: {
        type: 'proxy-shortcuts',
        orientation: 'horizontal',
        buttons: 'tun',
      },
      second: { type: 'proxy-shortcuts', orientation: 'vertical' },
    },
  })
  expect(
    getWidgetConfig(storage, 'first', WidgetId.ProxyShortcuts).orientation,
  ).toBe('horizontal')
  expect(
    getWidgetConfig(storage, 'second', WidgetId.ProxyShortcuts).orientation,
  ).toBe('vertical')
  expect(getWidgetConfig(storage, 'first', WidgetId.Memory)).toEqual(
    DEFAULT_WIDGET_CONFIGS[WidgetId.Memory],
  )
})

test('new references are validated and isolated while version 1 stays readable', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      quota: {
        type: WidgetId.SubscriptionQuota,
        target: { kind: 'fixed', profileUid: 'remote-a' },
        expiryWarningDays: 500,
        quotaWarningPercent: 0,
      },
      broken: {
        type: WidgetId.SubscriptionQuota,
        target: { kind: 'fixed', profileUid: '' },
        expiryWarningDays: '14',
        showProgress: false,
      },
      providers: {
        type: WidgetId.ProviderUpdates,
        resources: [
          { kind: 'proxy', name: 'same' },
          { kind: 'rule', name: 'same' },
          { kind: 'proxy', name: 'same' },
        ],
      },
      report: {
        type: WidgetId.OriginTraffic,
        range: 'last_hour',
        topN: 5,
        profileUid: 'deleted-profile',
        hideNames: true,
      },
      old: { type: WidgetId.TrafficDown, unit: 'bits' },
    },
  })
  expect(
    getWidgetConfig(storage, 'quota', WidgetId.SubscriptionQuota).target,
  ).toEqual({ kind: 'fixed', profileUid: 'remote-a' })
  expect(
    getWidgetConfig(storage, 'quota', WidgetId.SubscriptionQuota),
  ).toMatchObject({ expiryWarningDays: 365, quotaWarningPercent: 1 })
  expect(
    getWidgetConfig(storage, 'broken', WidgetId.SubscriptionQuota),
  ).toEqual({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
    showProgress: false,
  })
  expect(
    getWidgetConfig(storage, 'providers', WidgetId.ProviderUpdates).resources,
  ).toEqual([
    { kind: 'proxy', name: 'same' },
    { kind: 'rule', name: 'same' },
  ])
  expect(
    getWidgetConfig(storage, 'report', WidgetId.OriginTraffic),
  ).toMatchObject({
    range: 'last_hour',
    topN: 5,
    profileUid: 'deleted-profile',
    hideNames: true,
  })
  expect(getWidgetConfig(storage, 'old', WidgetId.TrafficDown).unit).toBe(
    'bits',
  )
})

test('numeric options clamp to their supported ranges and malformed values default per field', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      active: {
        type: WidgetId.ActiveConnections,
        sort: 'speed',
        topN: 100,
        showProcess: false,
        hideTargets: true,
      },
      resources: {
        type: WidgetId.ProviderUpdates,
        kinds: 'geo',
        maxItems: 0,
        resources: [{ kind: 'core', name: 'a' }],
      },
      history: { type: WidgetId.Memory, samples: 120 },
      report: {
        type: WidgetId.RecentTraffic,
        range: 'today',
        profileUid: 5,
        showDirections: false,
      },
    },
  })
  expect(
    getWidgetConfig(storage, 'active', WidgetId.ActiveConnections),
  ).toEqual({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.ActiveConnections],
    topN: 20,
    showProcess: false,
    hideTargets: true,
  })
  expect(
    getWidgetConfig(storage, 'resources', WidgetId.ProviderUpdates),
  ).toEqual({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.ProviderUpdates],
    maxItems: 1,
  })
  expect(getWidgetConfig(storage, 'history', WidgetId.Memory).samples).toBe(32)
  expect(getWidgetConfig(storage, 'report', WidgetId.RecentTraffic)).toEqual({
    ...DEFAULT_WIDGET_CONFIGS[WidgetId.RecentTraffic],
    showDirections: false,
  })
})
