import { expect, test } from 'vitest'
import {
  DEFAULT_WIDGET_CONFIGS,
  EMPTY_WIDGET_CONFIG_STORAGE,
  getWidgetConfig,
  normalizeWidgetConfigStorage,
  WidgetId,
} from '../src/pages/(main)/main/dashboard/_modules/widget-config'

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
