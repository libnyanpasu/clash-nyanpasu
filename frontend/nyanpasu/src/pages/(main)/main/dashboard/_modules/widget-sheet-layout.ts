import type { DndGridItemType } from '@nyanpasu/ui/dnd-grid'

type MinimumSize = { minW: number; minH: number }
type RecommendedSize = { w: number; h: number }

export function layoutWidgetSheet<T extends string>(
  ids: T[],
  cols: number,
  minimumSizes: Record<T, MinimumSize>,
  recommendedSizes: Partial<Record<T, RecommendedSize>>,
): DndGridItemType<T>[] {
  const columnHeights = Array<number>(cols).fill(0)
  const items: DndGridItemType<T>[] = []

  for (const id of ids) {
    const minimum = minimumSizes[id]
    const recommended = recommendedSizes[id]
    const size =
      recommended && recommended.w <= cols
        ? recommended
        : { w: minimum.minW, h: minimum.minH }
    if (size.w > cols) {
      continue
    }

    let x = 0
    let y = Number.POSITIVE_INFINITY

    for (let candidateX = 0; candidateX <= cols - size.w; candidateX++) {
      const candidateY = Math.max(
        ...columnHeights.slice(candidateX, candidateX + size.w),
      )
      if (candidateY < y) {
        x = candidateX
        y = candidateY
      }
    }

    items.push({ id, x, y, ...size })
    for (let column = x; column < x + size.w; column++) {
      columnHeights[column] = y + size.h
    }
  }

  return items
}
