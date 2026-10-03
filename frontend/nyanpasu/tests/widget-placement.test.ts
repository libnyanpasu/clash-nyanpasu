import { expect, test } from 'vitest'
import { WidgetId } from '@/components/widgets/widget-config'
import { placeWidget } from '../src/pages/(main)/main/dashboard/_modules/widget-placement'

const input = {
  id: 'new',
  type: WidgetId.SubscriptionQuota,
  cols: 6,
  rows: 4,
  minimum: { minW: 3, minH: 2 },
  recommended: { w: 4, h: 3 },
}

test('recommended size fits first; minimum is used when only a smaller opening exists', () => {
  expect(placeWidget({ ...input, items: [] })).toMatchObject({
    x: 0,
    y: 0,
    w: 4,
    h: 3,
  })
  expect(
    placeWidget({
      ...input,
      items: [
        { id: 'occupied', type: WidgetId.Memory, x: 3, y: 0, w: 3, h: 4 },
      ],
    }),
  ).toMatchObject({ x: 0, y: 0, w: 3, h: 2 })
})

test('full layouts append below existing items without overlap; narrow grids refuse an invalid width', () => {
  expect(
    placeWidget({
      ...input,
      items: [{ id: 'full', type: WidgetId.Memory, x: 0, y: 0, w: 6, h: 4 }],
    }),
  ).toMatchObject({ x: 0, y: 4, w: 3, h: 2 })
  expect(placeWidget({ ...input, cols: 2, items: [] })).toBeNull()
})
