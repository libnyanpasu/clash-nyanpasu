import { expect, test } from 'vitest'
import { layoutWidgetSheet } from '../src/pages/(main)/main/dashboard/_modules/widget-sheet-layout'

const minimumSizes = {
  large: { minW: 2, minH: 2 },
  'small-1': { minW: 2, minH: 2 },
  'small-2': { minW: 2, minH: 2 },
  'small-3': { minW: 2, minH: 2 },
  narrow: { minW: 4, minH: 2 },
}

test('keeps preferred widget spans and fills the lowest available skyline slot', () => {
  const items = layoutWidgetSheet(
    ['large', 'small-1', 'small-2', 'small-3'],
    6,
    minimumSizes,
    {
      large: { w: 4, h: 4 },
      'small-1': { w: 2, h: 2 },
      'small-2': { w: 2, h: 2 },
      'small-3': { w: 2, h: 2 },
    },
  )

  expect(items).toEqual([
    { id: 'large', x: 0, y: 0, w: 4, h: 4 },
    { id: 'small-1', x: 4, y: 0, w: 2, h: 2 },
    { id: 'small-2', x: 4, y: 2, w: 2, h: 2 },
    { id: 'small-3', x: 0, y: 4, w: 2, h: 2 },
  ])

  for (let leftIndex = 0; leftIndex < items.length; leftIndex++) {
    for (
      let rightIndex = leftIndex + 1;
      rightIndex < items.length;
      rightIndex++
    ) {
      const left = items[leftIndex]
      const right = items[rightIndex]
      const overlaps =
        left.x < right.x + right.w &&
        left.x + left.w > right.x &&
        left.y < right.y + right.h &&
        left.y + left.h > right.y
      expect(overlaps).toBe(false)
    }
  }
})

test('falls back to minimum size when a recommended width does not fit', () => {
  const items = layoutWidgetSheet(['large', 'narrow'], 3, minimumSizes, {
    large: { w: 4, h: 4 },
    narrow: { w: 6, h: 3 },
  })

  expect(items).toEqual([{ id: 'large', x: 0, y: 0, w: 2, h: 2 }])
})
