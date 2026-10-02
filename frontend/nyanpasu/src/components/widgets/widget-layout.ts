import type { DndGridItemType } from '@nyanpasu/ui/dnd-grid'
import type { DashboardItem } from './consts'
import { WidgetId } from './widget-config'

/** Older layouts can still contain widgets that have been removed. */
export function normalizeDashboardItems(
  items: (DndGridItemType<string> & { type?: string })[],
): DashboardItem[] {
  const supported = new Set<string>(Object.values(WidgetId))

  return items.flatMap((item) => {
    const type = item.type ?? item.id

    return supported.has(type) ? [{ ...item, type: type as WidgetId }] : []
  })
}
