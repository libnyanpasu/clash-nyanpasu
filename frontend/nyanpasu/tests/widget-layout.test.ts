import { expect, test } from 'vitest'
import { WidgetId } from '@/components/widgets/widget-config'
import { normalizeDashboardItems } from '@/components/widgets/widget-layout'

test('old layouts omit removed widgets and preserve remaining instance identity', () => {
  const geometry = { x: 0, y: 0, w: 4, h: 3 }
  const retained = {
    ...geometry,
    id: 'quota-instance',
    type: WidgetId.SubscriptionQuota,
  }

  expect(
    normalizeDashboardItems([
      { ...geometry, id: 'profile', type: 'profile-shortcuts' },
      { ...geometry, id: 'health', type: 'configuration-health' },
      { ...geometry, id: 'unknown', type: 'future-widget' },
      retained,
      { ...geometry, id: WidgetId.Memory },
    ]),
  ).toEqual([
    retained,
    { ...geometry, id: WidgetId.Memory, type: WidgetId.Memory },
  ])
})
