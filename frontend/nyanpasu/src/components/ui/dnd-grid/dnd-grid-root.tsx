import { useCallback, useRef, useState, type PropsWithChildren } from 'react'
import {
  DndContext,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from '@dnd-kit/core'
import {
  DndGridRootContext,
  type ActiveDrag,
  type GridRegistration,
} from './root-context'

function findGrid(
  grids: Map<string, GridRegistration>,
  activeId: string,
): { gridId: string; reg: GridRegistration; plainId: string } | null {
  for (const [gridId, reg] of grids) {
    const { itemIds, dragIdPrefix } = reg

    if (dragIdPrefix) {
      if (activeId.startsWith(dragIdPrefix)) {
        const plain = activeId.slice(dragIdPrefix.length)

        if (itemIds.includes(plain)) {
          return {
            gridId,
            reg,
            plainId: plain,
          }
        }
      }
    } else if (itemIds.includes(activeId)) {
      return {
        gridId,
        reg,
        plainId: activeId,
      }
    }
  }
  return null
}

export function DndGridRoot({ children }: PropsWithChildren) {
  const gridsRef = useRef<Map<string, GridRegistration>>(new Map())

  const [activeDrag, setActiveDrag] = useState<ActiveDrag | null>(null)

  // Captured at drag-start so the handler survives grid unmount (e.g. sheet closing)
  const pendingSourceRef = useRef<{
    onSourceDrop?: (itemId: string) => void
    itemId: string
  } | null>(null)

  const registerGrid = useCallback((gridId: string, reg: GridRegistration) => {
    gridsRef.current.set(gridId, reg)
  }, [])

  const unregisterGrid = useCallback((gridId: string) => {
    gridsRef.current.delete(gridId)
  }, [])

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, {
      activationConstraint: { delay: 200, tolerance: 6 },
    }),
  )

  const handleDragStart = useCallback((e: DragStartEvent) => {
    const activeId = String(e.active.id)
    const found = findGrid(gridsRef.current, activeId)
    if (!found) {
      return
    }

    const { reg, plainId } = found
    const { sourceOnly, getCellSize, onSourceDragStart } = reg

    const data = e.active.data.current as { w?: number; h?: number } | undefined
    const { cellW, cellH, gap } = getCellSize()
    const w = data?.w ?? 2
    const h = data?.h ?? 2

    if (sourceOnly) {
      // Capture before onSourceDragStart may unmount the grid
      pendingSourceRef.current = {
        onSourceDrop: reg.onSourceDrop,
        itemId: plainId,
      }
      onSourceDragStart?.()
    } else {
      reg.handleDragStart(e)
    }

    setActiveDrag({
      itemId: plainId,
      dims: {
        width: w * cellW + (w - 1) * gap,
        height: h * cellH + (h - 1) * gap,
      },
    })
  }, [])

  const handleDragMove = useCallback((e: DragMoveEvent) => {
    const activeId = String(e.active.id)
    const found = findGrid(gridsRef.current, activeId)
    if (!found) {
      return
    }

    const { reg } = found
    if (!reg.sourceOnly) {
      reg.handleDragMove(e)
    }
  }, [])

  const handleDragEnd = useCallback((e: DragEndEvent) => {
    setActiveDrag(null)

    // Source drag: use the captured handler (grid may have unmounted already)
    if (pendingSourceRef.current) {
      const { onSourceDrop, itemId } = pendingSourceRef.current
      pendingSourceRef.current = null
      onSourceDrop?.(itemId)
      return
    }

    const activeId = String(e.active.id)
    const found = findGrid(gridsRef.current, activeId)
    if (!found) {
      return
    }

    found.reg.handleDragEnd(e)
  }, [])

  const handleDragCancel = useCallback(() => {
    pendingSourceRef.current = null
    setActiveDrag(null)

    for (const [, reg] of gridsRef.current) {
      if (!reg.sourceOnly) {
        reg.handleDragCancel()
      }
    }
  }, [])

  return (
    <DndGridRootContext.Provider
      value={{ registerGrid, unregisterGrid, activeDrag }}
    >
      <DndContext
        sensors={sensors}
        onDragStart={handleDragStart}
        onDragMove={handleDragMove}
        onDragEnd={handleDragEnd}
        onDragCancel={handleDragCancel}
      >
        {children}
      </DndContext>
    </DndGridRootContext.Provider>
  )
}
