import { isEqual } from 'lodash-es'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { DndGridItemType, GridLayout, GridSize, ItemRect } from './types'

export function useGridLayout(
  minCellSize: number,
  gap: number,
  size?: GridSize,
  onSizeChange?: (size: GridSize) => void,
) {
  const containerRef = useRef<HTMLDivElement>(null)
  const onSizeChangeRef = useRef(onSizeChange)
  onSizeChangeRef.current = onSizeChange

  const [layout, setLayout] = useState<GridLayout>({
    cols: 1,
    rows: 1,
    cellW: minCellSize,
    cellH: minCellSize,
  })

  // Track container dimensions separately so we can recompute cellW/cellH when
  // the external `size` override changes without waiting for a resize event.
  const containerSizeRef = useRef({ width: 0, height: 0 })
  const lastComputedSizeRef = useRef<GridSize | null>(null)

  useEffect(() => {
    const el = containerRef.current
    if (!el) {
      return
    }

    const recalculate = (width: number, height: number) => {
      if (width <= 0 || height <= 0) {
        return
      }

      containerSizeRef.current = { width, height }

      // Number of cells that fit: solve `n * size + (n-1) * gap <= total`
      // => n <= (total + gap) / (size + gap)
      const computedCols = Math.max(
        1,
        Math.floor((width + gap) / (minCellSize + gap)),
      )
      const computedRows = Math.max(
        1,
        Math.floor((height + gap) / (minCellSize + gap)),
      )

      const computedSize = { cols: computedCols, rows: computedRows }
      if (!isEqual(computedSize, lastComputedSizeRef.current)) {
        lastComputedSizeRef.current = computedSize
        onSizeChangeRef.current?.(computedSize)
      }

      const cols = size?.cols ?? computedCols
      const rows = size?.rows ?? computedRows
      const cellW = (width - gap * (cols - 1)) / cols
      const cellH = (height - gap * (rows - 1)) / rows

      const nextLayout = { cols, rows, cellW, cellH }
      setLayout((prev) => (isEqual(prev, nextLayout) ? prev : nextLayout))
    }

    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect
      recalculate(width, height)
    })

    observer.observe(el)
    const { width, height } = el.getBoundingClientRect()
    recalculate(width, height)

    return () => {
      observer.disconnect()
    }
  }, [minCellSize, gap, size?.cols, size?.rows])

  const getItemRect = useCallback(
    (item: DndGridItemType): ItemRect => {
      const { cellW, cellH } = layout
      return {
        left: item.x * (cellW + gap),
        top: item.y * (cellH + gap),
        width: item.w * cellW + (item.w - 1) * gap,
        height: item.h * cellH + (item.h - 1) * gap,
      }
    },
    [layout, gap],
  )

  /** Convert a pixel delta into a clamped new grid position for the given item */
  const snapToGrid = useCallback(
    <T extends string>(
      item: DndGridItemType<T>,
      deltaX: number,
      deltaY: number,
    ): DndGridItemType<T> => {
      const { cellW, cellH, cols, rows } = layout
      const deltaCols = Math.round(deltaX / (cellW + gap))
      const deltaRows = Math.round(deltaY / (cellH + gap))

      return {
        ...item,
        x: Math.max(0, Math.min(cols - item.w, item.x + deltaCols)),
        y: Math.max(0, Math.min(rows - item.h, item.y + deltaRows)),
      }
    },
    [layout, gap],
  )

  return { containerRef, layout, getItemRect, snapToGrid }
}
