import { expect, test } from 'vitest'
import type {
  DashboardItem,
  LayoutStorage,
} from '../src/components/widgets/consts'
import { WidgetId } from '../src/components/widgets/widget-config'
import { adaptLayout } from '../src/pages/(main)/main/dashboard/_modules/layout-adapt'
import {
  effectiveLayoutSize,
  normalizeLayoutStorage,
  saveLayout,
  selectLayoutEntry,
} from '../src/pages/(main)/main/dashboard/_modules/layout-storage'

const item = (
  id: WidgetId,
  x: number,
  y: number,
  w: number,
  h: number,
): DashboardItem => ({ id, type: id, x, y, w, h })

const defaults: LayoutStorage = {
  '4x5': [item(WidgetId.TrafficDown, 0, 0, 2, 2)],
}

test('migration retains legacy layouts that share an effective footprint', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 2, 2)]
  const second = [item(WidgetId.TrafficUp, 0, 0, 2, 2)]

  const storage = normalizeLayoutStorage(
    { '4x5': first, '12x6': second },
    defaults,
  )

  expect(storage.entries).toEqual([
    { id: 'legacy:4x5', size: { cols: 2, rows: 2 }, items: first },
    { id: 'legacy:12x6', size: { cols: 2, rows: 2 }, items: second },
  ])
})

test('effective size ignores unused rows and columns in the legacy viewport key', () => {
  const items = [item(WidgetId.TrafficDown, 1, 2, 3, 2)]

  expect(effectiveLayoutSize(items)).toEqual({ cols: 4, rows: 4 })
  expect(
    normalizeLayoutStorage({ '12x8': items }, defaults).entries[0].size,
  ).toEqual({ cols: 4, rows: 4 })
})

test('selection honors the preferred edited entry', () => {
  const edited = [item(WidgetId.TrafficDown, 0, 0, 2, 2)]
  const other = [item(WidgetId.TrafficUp, 0, 0, 4, 4)]
  const storage = normalizeLayoutStorage(
    { '2x2': edited, '4x4': other },
    defaults,
  )
  const preferred = saveLayout(storage, 'legacy:2x2', edited)

  expect(selectLayoutEntry(preferred, { cols: 8, rows: 8 })?.id).toBe(
    'legacy:2x2',
  )
})

test('saving a colliding legacy entry preserves its sibling and round-trips preference', () => {
  const first = [item(WidgetId.TrafficDown, 0, 0, 2, 2)]
  const second = [item(WidgetId.TrafficUp, 0, 0, 2, 2)]
  const storage = normalizeLayoutStorage(
    { '4x5': first, '12x6': second },
    defaults,
  )
  const edited = [item(WidgetId.TrafficDown, 0, 0, 1, 2)]
  const saved = saveLayout(storage, 'legacy:4x5', edited)
  const roundTripped = normalizeLayoutStorage(saved, defaults)

  expect(roundTripped.preferredId).toBe('legacy:4x5')
  expect(roundTripped.entries).toEqual([
    {
      id: 'legacy:4x5',
      size: { cols: 1, rows: 2 },
      items: edited,
    },
    {
      id: 'legacy:12x6',
      size: { cols: 2, rows: 2 },
      items: second,
    },
  ])
})

test('v2 storage recomputes stale size and keeps its preferred entry even when it cannot fit', () => {
  const edited = [item(WidgetId.TrafficDown, 0, 0, 4, 3)]
  const storage = normalizeLayoutStorage(
    {
      version: 2,
      preferredId: 'edited',
      entries: [
        {
          id: 'edited',
          size: { cols: 1, rows: 1 },
          items: edited,
        },
        {
          id: 'fitting',
          size: { cols: 1, rows: 1 },
          items: [item(WidgetId.TrafficUp, 0, 0, 1, 1)],
        },
      ],
    },
    defaults,
  )

  expect(storage.entries[0].size).toEqual({ cols: 4, rows: 3 })
  expect(selectLayoutEntry(storage, { cols: 2, rows: 2 })?.id).toBe('edited')
})

test('an explicitly empty legacy layout stays empty', () => {
  const storage = normalizeLayoutStorage({ '4x5': [] }, defaults)

  expect(storage.entries).toEqual([
    { id: 'legacy:4x5', size: { cols: 1, rows: 1 }, items: [] },
  ])
})

test('an explicitly empty v2 layout stays empty', () => {
  const storage = normalizeLayoutStorage(
    {
      version: 2,
      preferredId: 'empty',
      entries: [{ id: 'empty', size: { cols: 10, rows: 10 }, items: [] }],
    },
    defaults,
  )

  expect(storage.preferredId).toBe('empty')
  expect(storage.entries).toEqual([
    { id: 'empty', size: { cols: 1, rows: 1 }, items: [] },
  ])
})

test('malformed legacy entries are ignored while valid entries survive', () => {
  const valid = [item(WidgetId.Memory, 0, 0, 2, 2)]
  const storage = normalizeLayoutStorage(
    {
      '4x5': valid,
      'not-a-size': valid,
      '8x6': [{ id: 'broken', type: WidgetId.Memory, x: 0 }],
      '6x6': [
        {
          id: WidgetId.Memory,
          type: WidgetId.Memory,
          x: Number.POSITIVE_INFINITY,
          y: 0,
          w: 2,
          h: 2,
        },
      ],
    },
    defaults,
  )

  expect(storage.entries).toEqual([
    { id: 'legacy:4x5', size: { cols: 2, rows: 2 }, items: valid },
  ])
})

test('migration restores id-only items and excludes removed types from the extent', () => {
  const storage = normalizeLayoutStorage(
    {
      '12x8': [
        { id: WidgetId.TrafficDown, x: 0, y: 0, w: 2, h: 2 },
        {
          id: 'removed-widget',
          type: 'removed-widget',
          x: 80,
          y: 80,
          w: 8,
          h: 8,
        },
      ],
    },
    defaults,
  )

  expect(storage.entries[0]).toEqual({
    id: 'legacy:12x8',
    size: { cols: 2, rows: 2 },
    items: [item(WidgetId.TrafficDown, 0, 0, 2, 2)],
  })
})

test('a narrow projection does not replace its source layout before growing again', () => {
  const sourceItems = [
    item(WidgetId.TrafficDown, 0, 0, 4, 2),
    item(WidgetId.TrafficUp, 4, 0, 2, 2),
  ]
  const storage = normalizeLayoutStorage({ '6x2': sourceItems }, defaults)
  const selected = selectLayoutEntry(storage, { cols: 2, rows: 2 })!
  const constraints = {
    [WidgetId.TrafficDown]: { minW: 2, minH: 2 },
    [WidgetId.TrafficUp]: { minW: 2, minH: 2 },
  }
  const narrowProjection = adaptLayout(
    selected.items,
    { cols: 2, rows: 2 },
    constraints,
  )

  expect(narrowProjection).toHaveLength(1)
  expect(selectLayoutEntry(storage, { cols: 6, rows: 2 })?.items).toEqual(
    sourceItems,
  )
  expect(
    adaptLayout(
      selectLayoutEntry(storage, { cols: 6, rows: 2 })!.items,
      { cols: 6, rows: 2 },
      constraints,
    ),
  ).toEqual(sourceItems)
})

test('adaptation relocates collisions without overlap or out-of-bounds items', () => {
  const source = [
    item(WidgetId.TrafficDown, 0, 0, 2, 2),
    item(WidgetId.TrafficUp, 0, 0, 2, 2),
    item(WidgetId.Memory, 0, 0, 2, 2),
  ]
  const adapted = adaptLayout(
    source,
    { cols: 4, rows: 2 },
    {
      [WidgetId.TrafficDown]: { minW: 2, minH: 2 },
      [WidgetId.TrafficUp]: { minW: 2, minH: 2 },
      [WidgetId.Memory]: { minW: 2, minH: 2 },
    },
  )

  expect(adapted).toHaveLength(2)
  for (const current of adapted) {
    expect(current.x).toBeGreaterThanOrEqual(0)
    expect(current.y).toBeGreaterThanOrEqual(0)
    expect(current.x + current.w).toBeLessThanOrEqual(4)
    expect(current.y + current.h).toBeLessThanOrEqual(2)
  }
  for (let index = 0; index < adapted.length; index++) {
    for (const other of adapted.slice(index + 1)) {
      const overlaps =
        adapted[index].x < other.x + other.w &&
        adapted[index].x + adapted[index].w > other.x &&
        adapted[index].y < other.y + other.h &&
        adapted[index].y + adapted[index].h > other.y
      expect(overlaps).toBe(false)
    }
  }
  expect(source).toEqual([
    item(WidgetId.TrafficDown, 0, 0, 2, 2),
    item(WidgetId.TrafficUp, 0, 0, 2, 2),
    item(WidgetId.Memory, 0, 0, 2, 2),
  ])
})
