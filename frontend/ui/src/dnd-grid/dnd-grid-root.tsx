import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PropsWithChildren,
} from 'react'
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

  // Keep the source identity when the library closes and unmounts its grid.
  const pendingSourceRef = useRef<{
    itemId: string
  } | null>(null)
  const pointerRef = useRef<{ x: number; y: number } | null>(null)

  useEffect(() => {
    const trackPointer = (event: PointerEvent) => {
      if (pendingSourceRef.current) {
        pointerRef.current = { x: event.clientX, y: event.clientY }
      }
    }
    const trackTouch = (event: TouchEvent) => {
      const touch = event.changedTouches[0]
      if (pendingSourceRef.current && touch) {
        pointerRef.current = { x: touch.clientX, y: touch.clientY }
      }
    }
    document.addEventListener('pointermove', trackPointer, true)
    document.addEventListener('pointerup', trackPointer, true)
    document.addEventListener('touchmove', trackTouch, true)
    document.addEventListener('touchend', trackTouch, true)
    return () => {
      document.removeEventListener('pointermove', trackPointer, true)
      document.removeEventListener('pointerup', trackPointer, true)
      document.removeEventListener('touchmove', trackTouch, true)
      document.removeEventListener('touchend', trackTouch, true)
    }
  }, [])

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
      pointerRef.current = null
      // Capture before onSourceDragStart may unmount the grid
      pendingSourceRef.current = {
        itemId: plainId,
      }
      onSourceDragStart?.()
    } else {
      reg.handleDragStart(e)
    }

    setActiveDrag({
      itemId: plainId,
      dragIdPrefix: reg.dragIdPrefix,
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

    // The destination owns the drop; a missing destination cancels the addition.
    if (pendingSourceRef.current) {
      const { itemId } = pendingSourceRef.current
      pendingSourceRef.current = null

      // Drag deltas include layout adjustments when the drawer closes; use
      // the native release coordinates instead.
      const pointer = pointerRef.current
      pointerRef.current = null
      if (pointer) {
        for (const reg of gridsRef.current.values()) {
          if (reg.sourceOnly || !reg.onExternalDrop) continue
          const position = reg.getDropPosition(pointer.x, pointer.y)
          if (position) {
            reg.onExternalDrop(itemId, position)
            break
          }
        }
      }
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
    pointerRef.current = null
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
