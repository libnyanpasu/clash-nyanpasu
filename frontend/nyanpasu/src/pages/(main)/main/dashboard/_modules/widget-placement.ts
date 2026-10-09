import { hasOverlap } from '@nyanpasu/ui/dnd-grid'
import type { DashboardItem } from '@/components/widgets/consts'
import type { WidgetId } from '@/components/widgets/widget-config'

export function placeWidget({
  id,
  type,
  items,
  cols,
  rows,
  minimum,
  recommended,
  position,
}: {
  id: string
  type: WidgetId
  items: DashboardItem[]
  cols: number
  rows: number
  minimum: { minW: number; minH: number }
  recommended?: { w: number; h: number }
  position?: { x: number; y: number }
}): DashboardItem | null {
  if (cols < minimum.minW) return null
  const sizes = [recommended, { w: minimum.minW, h: minimum.minH }].filter(
    (size) => size !== undefined,
  )
  for (const { w, h } of sizes) {
    if (position) {
      if (w > cols || h > rows) continue
      const candidate = {
        id,
        type,
        x: Math.max(0, Math.min(cols - w, position.x)),
        y: Math.max(0, Math.min(rows - h, position.y)),
        w,
        h,
      }
      if (!hasOverlap(items, id, candidate)) return candidate
      continue
    }

    for (let y = 0; y <= rows - h; y++) {
      for (let x = 0; x <= cols - w; x++) {
        const candidate = { id, type, x, y, w, h }
        if (!hasOverlap(items, id, candidate)) return candidate
      }
    }
  }
  if (position) return null
  return {
    id,
    type,
    x: 0,
    y: items.reduce((end, item) => Math.max(end, item.y + item.h), 0),
    w: minimum.minW,
    h: minimum.minH,
  }
}
