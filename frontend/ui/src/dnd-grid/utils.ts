import type {
  DndGridItemType,
  GridItemConstraints,
  ResizeHandle,
} from './types'

export function isOverlap<T extends string>(
  a: DndGridItemType<T>,
  b: DndGridItemType<T>,
): boolean {
  return (
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
  )
}

export function hasOverlap<T extends string>(
  items: DndGridItemType<T>[],
  movingId: string,
  candidate: DndGridItemType<T>,
): boolean {
  return items.some(
    (item) => item.id !== movingId && isOverlap(candidate, item),
  )
}

export function calculateResize<T extends string>(
  startItem: DndGridItemType<T>,
  handle: ResizeHandle,
  deltaX: number,
  deltaY: number,
  cellW: number,
  cellH: number,
  gap: number,
  cols: number,
  rows: number,
  constraints: GridItemConstraints = {},
): DndGridItemType<T> {
  const minW = constraints.minW ?? 1
  const minH = constraints.minH ?? 1
  const maxW = constraints.maxW ?? cols
  const maxH = constraints.maxH ?? rows
  const stepX = cellW + gap
  const stepY = cellH + gap
  const deltaCols = Math.round(deltaX / stepX)
  const deltaRows = Math.round(deltaY / stepY)

  let { x, y, w, h } = startItem

  if (handle.includes('right')) {
    w = Math.max(minW, Math.min(maxW, cols - x, w + deltaCols))
  }

  if (handle.includes('bottom')) {
    h = Math.max(minH, Math.min(maxH, rows - y, h + deltaRows))
  }

  if (handle.includes('left')) {
    const newX = Math.max(0, Math.min(x + w - minW, x + deltaCols))
    const newW = Math.min(maxW, w + (x - newX))
    x = newX + (w + (x - newX) - newW)
    w = newW
  }

  if (handle.includes('top')) {
    const newY = Math.max(0, Math.min(y + h - minH, y + deltaRows))
    const newH = Math.min(maxH, h + (y - newY))
    y = newY + (h + (y - newY) - newH)
    h = newH
  }

  return {
    ...startItem,
    x,
    y,
    w,
    h,
  }
}
