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

test('a drop uses its requested cell even when earlier cells are free', () => {
  expect(
    placeWidget({ ...input, items: [], position: { x: 2, y: 1 } }),
  ).toMatchObject({ x: 2, y: 1, w: 4, h: 3 })
})

test('a drop tries the minimum size at its cell before searching elsewhere', () => {
  expect(
    placeWidget({
      ...input,
      position: { x: 0, y: 0 },
      items: [
        { id: 'occupied', type: WidgetId.Memory, x: 3, y: 0, w: 3, h: 2 },
      ],
    }),
  ).toMatchObject({ x: 0, y: 0, w: 3, h: 2 })
})

test('an occupied drop is refused instead of being moved to a free cell or appended', () => {
  for (const w of [3, 6]) {
    expect(
      placeWidget({
        ...input,
        position: { x: 0, y: 0 },
        items: [{ id: 'occupied', type: WidgetId.Memory, x: 0, y: 0, w, h: 4 }],
      }),
    ).toBeNull()
  }
})
