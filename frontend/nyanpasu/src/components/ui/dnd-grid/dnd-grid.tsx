import { AnimatePresence, motion } from 'framer-motion'
import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from '@dnd-kit/core'
import { cn } from '@nyanpasu/ui'
import { DndGridProvider } from './context'
import { useDndGridRoot, type GridRegistration } from './root-context'
import type {
  DndGridItemType,
  GridItemConstraints,
  GridSize,
  ResizeHandle,
} from './types'
import { useGridLayout } from './use-grid-layout'
import { calculateResize, hasOverlap } from './utils'

export interface DndGridProps<T extends string = string> {
  items: DndGridItemType<T>[]
  onLayoutChange?: (items: DndGridItemType<T>[]) => void
  minCellSize?: number
  gap?: number
  size?: GridSize
  onSizeChange?: (
    size: GridSize,
    constraintsMap: Record<string, GridItemConstraints>,
  ) => void
  children: (item: DndGridItemType<T>) => React.ReactNode
  className?: string
  disabled?: boolean
  sourceOnly?: boolean
  dragIdPrefix?: string
  gridId?: string
  onSourceDrop?: (itemId: string) => void
  onSourceDragStart?: () => void
}

export function DndGrid<T extends string = string>({
  items,
  onLayoutChange,
  minCellSize = 96,
  gap = 8,
  size,
  onSizeChange,
  children,
  className,
  disabled = true,
  sourceOnly = false,
  dragIdPrefix = '',
  gridId,
  onSourceDrop,
  onSourceDragStart,
}: DndGridProps<T>) {
  const constraintsMapRef = useRef<Record<string, GridItemConstraints>>({})

  const { containerRef, layout, computedSize, getItemRect, snapToGrid } =
    useGridLayout(minCellSize, gap, size)

  const onSizeChangeRef = useRef(onSizeChange)
  onSizeChangeRef.current = onSizeChange

  useEffect(() => {
    if (computedSize) {
      onSizeChangeRef.current?.(computedSize, constraintsMapRef.current)
    }
  }, [computedSize])

  const [activeItem, setActiveItem] = useState<DndGridItemType<T> | null>(null)
  const [previewItem, setPreviewItem] = useState<DndGridItemType<T> | null>(
    null,
  )

  const [displayItems, setDisplayItems] = useState<DndGridItemType<T>[]>(items)
  const [dropInfoMap, setDropInfoMap] = useState<
    Record<string, { left: number; top: number }>
  >({})

  const isDragging = useRef(false)
  const lastValidSnapRef = useRef<DndGridItemType<T> | null>(null)

  const resizeStateRef = useRef<{
    id: string
    handle: ResizeHandle
    startItem: DndGridItemType<T>
    startX: number
    startY: number
  } | null>(null)
  const resizePreviewRef = useRef<DndGridItemType<T> | null>(null)

  const [resizingItemId, setResizingItemId] = useState<string | null>(null)
  const [resizePreview, setResizePreview] = useState<DndGridItemType<T> | null>(
    null,
  )

  useEffect(() => {
    if (!isDragging.current && !resizeStateRef.current) {
      setDisplayItems(items)
    }
  }, [items])

  const effectiveDisplayItems = resizePreview
    ? displayItems.map((item) =>
        item.id === resizePreview.id ? resizePreview : item,
      )
    : displayItems

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, {
      activationConstraint: { delay: 200, tolerance: 6 },
    }),
  )

  const handleDragStart = useCallback(
    ({ active }: DragStartEvent) => {
      const item = items.find((i) => i.id === active.id)
      if (!item) {
        return
      }

      isDragging.current = true
      lastValidSnapRef.current = item
      setActiveItem(item)
      setPreviewItem(item)
      setDisplayItems(items)
    },
    [items],
  )

  const handleDragMove = useCallback(
    ({ delta }: DragMoveEvent) => {
      if (!activeItem) {
        return
      }

      const snapped = snapToGrid(activeItem, delta.x, delta.y)

      // Only update the placeholder when the target cell is free.
      if (!hasOverlap(items, activeItem.id, snapped)) {
        lastValidSnapRef.current = snapped
        setPreviewItem(snapped)
      }
      // If overlapping, placeholder stays at lastValidSnapRef — no state update needed.
    },
    [activeItem, items, snapToGrid],
  )

  const handleDragEnd = useCallback(
    ({ active, delta }: DragEndEvent) => {
      if (!activeItem) {
        return
      }

      const originalRect = getItemRect(activeItem)
      const dropLeft = originalRect.left + delta.x
      const dropTop = originalRect.top + delta.y

      const snapped = snapToGrid(activeItem, delta.x, delta.y)
      // Use the snapped position if it's free, otherwise fall back to the last valid snap.
      const finalItem = !hasOverlap(items, activeItem.id, snapped)
        ? snapped
        : (lastValidSnapRef.current ?? activeItem)

      const newItems = items.map((i) => (i.id === active.id ? finalItem : i))
      const id = String(active.id)

      isDragging.current = false
      lastValidSnapRef.current = null

      setActiveItem(null)
      setPreviewItem(null)
      setDisplayItems(newItems)
      setDropInfoMap((prev) => ({
        ...prev,
        [id]: { left: dropLeft, top: dropTop },
      }))
      onLayoutChange?.(newItems)
    },
    [activeItem, items, snapToGrid, getItemRect, onLayoutChange],
  )

  const handleDragCancel = useCallback(() => {
    isDragging.current = false
    lastValidSnapRef.current = null
    setActiveItem(null)
    setPreviewItem(null)
    setDisplayItems(items)
  }, [items])

  const onResizeStart = useCallback(
    (id: string, handle: ResizeHandle, startX: number, startY: number) => {
      const item = items.find((i) => i.id === id)
      if (!item || disabled) {
        return
      }

      resizeStateRef.current = {
        id,
        handle,
        startItem: item,
        startX,
        startY,
      }

      setResizingItemId(id)
    },
    [items, disabled],
  )

  const onResizeMove = useCallback(
    (currentX: number, currentY: number) => {
      const state = resizeStateRef.current
      if (!state) {
        return
      }

      const { cellW, cellH, cols, rows } = layout
      const deltaX = currentX - state.startX
      const deltaY = currentY - state.startY
      const candidate = calculateResize(
        state.startItem,
        state.handle,
        deltaX,
        deltaY,
        cellW,
        cellH,
        gap,
        cols,
        rows,
        constraintsMapRef.current[state.id],
      )

      if (!hasOverlap(items, state.id, candidate)) {
        resizePreviewRef.current = candidate
        setResizePreview(candidate)
      }
    },
    [items, layout, gap],
  )

  const onResizeEnd = useCallback(() => {
    const preview = resizePreviewRef.current
    resizeStateRef.current = null
    resizePreviewRef.current = null
    setResizingItemId(null)
    setResizePreview(null)

    if (preview) {
      const newItems = items.map((i) => (i.id === preview.id ? preview : i))
      setDisplayItems(newItems)
      onLayoutChange?.(newItems)
    }
  }, [items, onLayoutChange])

  const rootCtx = useDndGridRoot()

  // Stable object mutated in place every render so the root always reads fresh closures
  const registrationRef = useRef<GridRegistration>({
    itemIds: [],
    dragIdPrefix: '',
    sourceOnly: false,
    handleDragStart: () => {},
    handleDragMove: () => {},
    handleDragEnd: () => {},
    handleDragCancel: () => {},
    getCellSize: () => ({ cellW: 0, cellH: 0, gap: 0 }),
  })

  Object.assign(registrationRef.current, {
    itemIds: items.map((i) => i.id),
    dragIdPrefix,
    sourceOnly,
    handleDragStart,
    handleDragMove,
    handleDragEnd,
    handleDragCancel,
    getCellSize: () => ({ cellW: layout.cellW, cellH: layout.cellH, gap }),
    onSourceDrop,
    onSourceDragStart,
  })

  useLayoutEffect(() => {
    if (!rootCtx || !gridId) {
      return
    }

    rootCtx.registerGrid(gridId, registrationRef.current)

    return () => {
      rootCtx.unregisterGrid(gridId)
    }
  }, [rootCtx, gridId])

  const isManaged = Boolean(rootCtx && gridId)

  const overlayRect = activeItem ? getItemRect(activeItem) : null
  const placeholderRect = previewItem ? getItemRect(previewItem) : null

  const gridContent = (
    <DndGridProvider
      value={{
        displayItems: effectiveDisplayItems,
        getItemRect,
        dropInfoMap,
        activeItemId: activeItem?.id ?? null,
        resizingItemId,
        disabled,
        sourceOnly,
        dragIdPrefix,
        isOverlay: false,
        constraintsMapRef,
        onResizeStart,
        onResizeMove,
        onResizeEnd,
      }}
    >
      <div
        ref={containerRef}
        className={cn('relative', className)}
        data-slot="dnd-grid-container"
      >
        <AnimatePresence>
          {placeholderRect && activeItem && (
            <motion.div
              key="dnd-grid-placeholder"
              data-slot="dnd-grid-placeholder"
              layout
              className={cn(
                'border-primary/40 bg-primary/5 border-2 border-dashed',
                'pointer-events-none absolute rounded-2xl',
              )}
              style={{
                left: placeholderRect.left,
                top: placeholderRect.top,
                width: placeholderRect.width,
                height: placeholderRect.height,
              }}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ type: 'spring', stiffness: 300, damping: 28 }}
            />
          )}
        </AnimatePresence>

        {effectiveDisplayItems.map((item) => (
          <Fragment key={item.id}>{children(item)}</Fragment>
        ))}
      </div>

      {!isManaged && (
        <DragOverlay dropAnimation={null}>
          <AnimatePresence>
            {activeItem && overlayRect && (
              <motion.div
                key="dnd-grid-overlay"
                data-slot="dnd-grid-overlay"
                className="cursor-grabbing"
                style={{ width: overlayRect.width, height: overlayRect.height }}
                initial={{ opacity: 0.85 }}
                animate={{ opacity: 0.95 }}
                exit={{ opacity: 0 }}
                transition={{ type: 'tween', duration: 0.1, ease: 'easeOut' }}
              >
                <DndGridProvider
                  value={{
                    displayItems: effectiveDisplayItems,
                    getItemRect,
                    dropInfoMap,
                    activeItemId: activeItem?.id ?? null,
                    resizingItemId,
                    disabled,
                    sourceOnly,
                    dragIdPrefix,
                    isOverlay: true,
                    constraintsMapRef,
                    onResizeStart,
                    onResizeMove,
                    onResizeEnd,
                  }}
                >
                  {children(activeItem)}
                </DndGridProvider>
              </motion.div>
            )}
          </AnimatePresence>
        </DragOverlay>
      )}
    </DndGridProvider>
  )

  if (isManaged) {
    return gridContent
  }

  return (
    <DndContext
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragMove={handleDragMove}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      {gridContent}
    </DndContext>
  )
}
