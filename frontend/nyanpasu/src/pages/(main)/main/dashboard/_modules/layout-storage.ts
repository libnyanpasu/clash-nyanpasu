import type { GridSize } from '@nyanpasu/ui/dnd-grid'
import type { DashboardItem, LayoutStorage } from '@/components/widgets/consts'
import { normalizeDashboardItems } from '@/components/widgets/widget-layout'

export type DashboardLayoutEntry = {
  // Identity stays stable even when legacy layouts share the same footprint.
  id: string
  size: GridSize
  items: DashboardItem[]
}

export type DashboardLayoutStorage = {
  version: 2
  preferredId: string | null
  entries: DashboardLayoutEntry[]
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

type LegacyDashboardItem = {
  id: string
  type?: string
  x: number
  y: number
  w: number
  h: number
}

const isItem = (value: unknown): value is LegacyDashboardItem => {
  if (!isRecord(value)) return false

  return (
    typeof value.id === 'string' &&
    (value.type === undefined || typeof value.type === 'string') &&
    Number.isInteger(value.x) &&
    Number.isInteger(value.y) &&
    Number.isInteger(value.w) &&
    Number.isInteger(value.h) &&
    (value.x as number) >= 0 &&
    (value.y as number) >= 0 &&
    (value.w as number) > 0 &&
    (value.h as number) > 0
  )
}

const normalizeItems = (value: unknown): DashboardItem[] | null => {
  if (!Array.isArray(value) || !value.every(isItem)) return null
  return normalizeDashboardItems(value)
}

export function effectiveLayoutSize(items: DashboardItem[]): GridSize {
  return items.reduce(
    (size, item) => ({
      cols: Math.max(size.cols, item.x + item.w),
      rows: Math.max(size.rows, item.y + item.h),
    }),
    { cols: 1, rows: 1 },
  )
}

function fromLegacyLayouts(
  value: Record<string, unknown>,
): DashboardLayoutEntry[] {
  return Object.entries(value).flatMap(([key, items]) => {
    const normalizedItems = normalizeItems(items)
    if (!/^\d+x\d+$/.test(key) || !normalizedItems) return []

    return [
      {
        id: `legacy:${key}`,
        size: effectiveLayoutSize(normalizedItems),
        items: normalizedItems,
      },
    ]
  })
}

/**
 * Convert old dimension maps to v2 while preserving every valid legacy entry.
 * Reads migrate in memory; the next explicit edit writes v2 to storage.
 */
export function normalizeLayoutStorage(
  value: unknown,
  defaults: LayoutStorage,
): DashboardLayoutStorage {
  if (isRecord(value) && value.version === 2 && Array.isArray(value.entries)) {
    const entries = value.entries.flatMap((entry): DashboardLayoutEntry[] => {
      if (!isRecord(entry) || typeof entry.id !== 'string') return []
      const items = normalizeItems(entry.items)
      if (!items) return []

      return [
        {
          id: entry.id,
          size: effectiveLayoutSize(items),
          items,
        },
      ]
    })
    const uniqueEntries = entries.filter(
      (entry, index) =>
        entries.findIndex(({ id }) => id === entry.id) === index,
    )
    const preferredId =
      typeof value.preferredId === 'string' &&
      uniqueEntries.some(({ id }) => id === value.preferredId)
        ? value.preferredId
        : null

    return {
      version: 2,
      preferredId,
      entries: uniqueEntries.length
        ? uniqueEntries
        : fromLegacyLayouts(defaults),
    }
  }

  const legacy = isRecord(value) ? fromLegacyLayouts(value) : []
  return {
    version: 2,
    preferredId: null,
    entries: legacy.length ? legacy : fromLegacyLayouts(defaults),
  }
}

export function selectLayoutEntry(
  storage: DashboardLayoutStorage,
  availableSize: GridSize,
): DashboardLayoutEntry | null {
  const preferred = storage.entries.find(({ id }) => id === storage.preferredId)
  if (preferred) return preferred

  let best: { entry: DashboardLayoutEntry; score: number } | null = null
  for (const entry of storage.entries) {
    const { cols, rows } = entry.size
    const fits = cols <= availableSize.cols && rows <= availableSize.rows
    const score = fits
      ? -(cols * rows)
      : Math.abs(cols - availableSize.cols) +
        Math.abs(rows - availableSize.rows)

    if (!best || score < best.score) best = { entry, score }
  }

  return best?.entry ?? null
}

export function saveLayout(
  storage: DashboardLayoutStorage,
  sourceId: string,
  items: DashboardItem[],
): DashboardLayoutStorage {
  const entry: DashboardLayoutEntry = {
    id: sourceId,
    size: effectiveLayoutSize(items),
    items,
  }
  const existingIndex = storage.entries.findIndex(({ id }) => id === sourceId)

  return {
    version: 2,
    preferredId: sourceId,
    entries:
      existingIndex < 0
        ? [...storage.entries, entry]
        : storage.entries.map((current, index) =>
            index === existingIndex ? entry : current,
          ),
  }
}
