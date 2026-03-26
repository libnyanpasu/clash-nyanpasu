import type {
  DndGridItemType,
  GridItemConstraints,
  GridSize,
} from '@/components/ui/dnd-grid'
import { isOverlap } from '@/components/ui/dnd-grid/utils'

export function sizeKey(size: GridSize): string {
  return `${size.cols}x${size.rows}`
}

/**
 * Find the best stored layout for a given grid size.
 * Scans all stored layouts whose dimensions fit within `size` and returns the
 * one with the largest area (closest match). Returns null if none found.
 */
export function findBestLayout<T extends DndGridItemType<string>>(
  storage: Record<string, T[]>,
  size: GridSize,
): T[] | null {
  let best: { area: number; items: T[] } | null = null

  for (const [key, items] of Object.entries(storage)) {
    const match = key.match(/^(\d+)x(\d+)$/)
    if (!match) continue

    const cols = parseInt(match[1], 10)
    const rows = parseInt(match[2], 10)

    if (cols <= size.cols && rows <= size.rows) {
      const area = cols * rows

      if (!best || area > best.area) {
        best = { area, items }
      }
    }
  }

  return best?.items ?? null
}

/**
 * When no layout fits within `size`, find the stored layout whose dimensions
 * are closest (Manhattan distance on cols/rows) to use as an adaptation base.
 * Returns null if storage is empty.
 */
export function findClosestStoredLayout<T extends DndGridItemType<string>>(
  storage: Record<string, T[]>,
  size: GridSize,
): T[] | null {
  let best: { dist: number; items: T[] } | null = null

  for (const [key, items] of Object.entries(storage)) {
    const match = key.match(/^(\d+)x(\d+)$/)
    if (!match) continue

    const cols = parseInt(match[1], 10)
    const rows = parseInt(match[2], 10)
    const dist = Math.abs(cols - size.cols) + Math.abs(rows - size.rows)

    if (!best || dist < best.dist) {
      best = { dist, items }
    }
  }

  return best?.items ?? null
}

function hasOverlapWith<T extends DndGridItemType<string>>(
  placed: T[],
  candidate: T,
): boolean {
  return placed.some((p) => p.id !== candidate.id && isOverlap(p, candidate))
}

/** Scan top-to-bottom, left-to-right for the first free slot of size (w × h). */
function tryPlace<T extends DndGridItemType<string>>(
  item: T,
  w: number,
  h: number,
  placed: T[],
  cols: number,
  rows: number,
): T | null {
  for (let y = 0; y + h <= rows; y++) {
    for (let x = 0; x + w <= cols; x++) {
      const candidate = { ...item, x, y, w, h } as T

      if (!hasOverlapWith(placed, candidate)) {
        return candidate
      }
    }
  }
  return null
}

/**
 * Adapt `items` so they all fit within the new `size`.
 *
 * Priority per item:
 *   1. Clamp (x,y) so the item stays in-bounds with its current (w,h).
 *   2. If that position overlaps others, scan for a free slot at the same size.
 *   3. If still no slot, progressively shrink (w,h) toward (minW,minH) and
 *      scan again.
 *   4. If even the minimum size can't be placed, drop the item.
 *
 * Items that are already within bounds and overlap-free are left unchanged.
 * Items are processed in reading order (top → bottom, left → right) so earlier
 * items have priority over later ones.
 */
export function adaptLayout<T extends DndGridItemType<string>>(
  items: T[],
  size: GridSize,
  constraints: Record<string, GridItemConstraints>,
): T[] {
  const { cols, rows } = size
  const result: T[] = []

  const sorted = [...items].sort((a, b) =>
    a.y !== b.y ? a.y - b.y : a.x - b.x,
  )

  for (const item of sorted) {
    const c = constraints[item.id] ?? {}
    const minW = c.minW ?? 1
    const minH = c.minH ?? 1

    // Can't fit even at minimum size — drop.
    if (minW > cols || minH > rows) continue

    // Clamp dimensions to [minW..cols] and [minH..rows].
    const w = Math.max(minW, Math.min(item.w, cols))
    const h = Math.max(minH, Math.min(item.h, rows))
    // Clamp position so the item stays fully in-bounds.
    const x = Math.max(0, Math.min(item.x, cols - w))
    const y = Math.max(0, Math.min(item.y, rows - h))

    const clamped = { ...item, x, y, w, h } as T

    // Step 1: try at clamped position (no overlap).
    if (!hasOverlapWith(result, clamped)) {
      result.push(clamped)
      continue
    }

    // Step 2: find a free slot at current (w, h).
    const placed = tryPlace(item, w, h, result, cols, rows)
    if (placed) {
      result.push(placed)
      continue
    }

    // Step 3: shrink (w, h) toward (minW, minH) and retry.
    const findShrinkPlacement = (): T | null => {
      for (let tw = w; tw >= minW; tw--) {
        for (let th = h; th >= minH; th--) {
          if (tw === w && th === h) continue // already tried above

          const p = tryPlace(item, tw, th, result, cols, rows)

          if (p) {
            return p
          }
        }
      }

      return null
    }

    const found = findShrinkPlacement()
    if (found) result.push(found)
    // else: drop the item.
  }

  return result
}
