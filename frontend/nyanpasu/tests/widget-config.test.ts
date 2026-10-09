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
    getWidgetConfig(storage, 'first', WidgetId.ProxyShortcuts).buttons,
  ).toBe('tun')
  expect(
    getWidgetConfig(storage, 'second', WidgetId.ProxyShortcuts).buttons,
  ).toBe('both')
  expect(
    getWidgetConfig(storage, 'second', WidgetId.ProxyShortcuts),
  ).not.toHaveProperty('orientation')
  expect(getWidgetConfig(storage, 'first', WidgetId.Memory)).toEqual(
    DEFAULT_WIDGET_CONFIGS[WidgetId.Memory],
  )
})

test('proxy mode layouts persist per instance and old or invalid choices use the flex layout', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      focus: { type: WidgetId.ProxyMode, layout: 'focus' },
      flex: { type: WidgetId.ProxyMode, layout: 'flex' },
      old: { type: WidgetId.ProxyMode },
      invalid: { type: WidgetId.ProxyMode, layout: 'tabs' },
    },
  })

  expect(getWidgetConfig(storage, 'flex', WidgetId.ProxyMode).layout).toBe(
    'flex',
  )
  expect(getWidgetConfig(storage, 'focus', WidgetId.ProxyMode).layout).toBe(
    'focus',
  )
  for (const id of ['old', 'invalid']) {
    expect(getWidgetConfig(storage, id, WidgetId.ProxyMode).layout).toBe('flex')
  }
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

test('quota wave options restore saved choices and keep defaults for older or invalid storage', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      old: { type: WidgetId.SubscriptionQuota },
      single: {
        type: WidgetId.SubscriptionQuota,
        waveStyle: 'single',
        animateWave: false,
      },
      invalid: {
        type: WidgetId.SubscriptionQuota,
        waveStyle: 'triple',
        animateWave: 'false',
      },
    },
  })
  expect(getWidgetConfig(storage, 'old', WidgetId.SubscriptionQuota)).toEqual(
    DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota],
  )
  expect(
    getWidgetConfig(storage, 'invalid', WidgetId.SubscriptionQuota),
  ).toEqual(DEFAULT_WIDGET_CONFIGS[WidgetId.SubscriptionQuota])
  expect(
    getWidgetConfig(storage, 'single', WidgetId.SubscriptionQuota),
  ).toMatchObject({ waveStyle: 'single', animateWave: false })
})

test('both proxy widgets default to flex while status preserves an explicit equal layout', () => {
  const storage = normalizeWidgetConfigStorage({
    version: 1,
    byInstance: {
      status: { type: WidgetId.ProxyShortcuts },
      equal: { type: WidgetId.ProxyShortcuts, layout: 'equal' },
      invalid: { type: WidgetId.ProxyShortcuts, layout: 'focus' },
      mode: { type: WidgetId.ProxyMode },
    },
  })
  for (const id of ['status', 'invalid'])
    expect(getWidgetConfig(storage, id, WidgetId.ProxyShortcuts).layout).toBe(
      'flex',
    )
  expect(
    getWidgetConfig(storage, 'equal', WidgetId.ProxyShortcuts).layout,
  ).toBe('equal')
  expect(getWidgetConfig(storage, 'mode', WidgetId.ProxyMode).layout).toBe(
    'flex',
  )
})
