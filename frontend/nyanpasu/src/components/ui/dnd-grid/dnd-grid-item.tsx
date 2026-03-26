import {
  AnimatePresence,
  motion,
  useSpring,
  type Transition,
} from 'framer-motion'
import { PropsWithChildren, useLayoutEffect, useRef } from 'react'
import { useDraggable } from '@dnd-kit/core'
import { cn } from '@nyanpasu/ui'
import { useDndGridContext } from './context'
import type { GridItemConstraints } from './types'

const SPRING_OPTIONS = {
  stiffness: 350,
  damping: 35,
} as Transition

const RESIZE_SPRING = {
  type: 'spring',
  stiffness: 400,
  damping: 35,
} as Transition

const INSTANT = {
  duration: 0,
} as Transition

export type DndGridItemProps = PropsWithChildren<{
  id: string
  className?: string
}> &
  GridItemConstraints

function ResizeKnob({
  onStart,
  onMove,
  onEnd,
}: {
  onStart: (x: number, y: number) => void
  onMove: (x: number, y: number) => void
  onEnd: () => void
}) {
  return (
    <motion.div
      className={cn(
        'absolute -right-0.75 -bottom-0.75 z-20 flex size-7 items-center justify-center',
        'text-on-surface',
        'touch-none select-none',
      )}
      data-slot="resize-handle"
      onPointerDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
        e.currentTarget.setPointerCapture(e.pointerId)
        onStart(e.clientX, e.clientY)
      }}
      onPointerMove={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return
        }

        onMove(e.clientX, e.clientY)
      }}
      onPointerUp={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return
        }

        e.currentTarget.releasePointerCapture(e.pointerId)
        onEnd()
      }}
      onPointerCancel={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) {
          return
        }

        e.currentTarget.releasePointerCapture(e.pointerId)
        onEnd()
      }}
      initial={{
        scale: 0.85,
        opacity: 0,
      }}
      animate={{
        scale: 1,
        opacity: 1,
      }}
      exit={{
        scale: 0.85,
        opacity: 0,
      }}
      transition={{
        type: 'tween',
        duration: 0.1,
        ease: 'easeOut',
      }}
    >
      <svg
        className="size-full cursor-se-resize"
        viewBox="11 11 7 7"
        fill="none"
        data-slot="resize-handle-icon"
      >
        <path
          d="M12 17.25H13A4.25 4.25 0 0 0 17.25 13V12"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
    </motion.div>
  )
}

function DndGridItemDraggable({
  id,
  className,
  children,
  minW,
  minH,
  maxW,
  maxH,
}: DndGridItemProps) {
  const {
    displayItems,
    getItemRect,
    dropInfoMap,
    activeItemId,
    resizingItemId,
    disabled,
    constraintsMapRef,
    onResizeStart,
    onResizeMove,
    onResizeEnd,
  } = useDndGridContext()

  // Write constraints synchronously during render so they're always up-to-date
  // before any resize interaction can occur.
  constraintsMapRef.current[id] = { minW, minH, maxW, maxH }

  const item = displayItems.find((i) => i.id === id)

  // Disable drag while any item is being resized
  const { attributes, listeners, setNodeRef } = useDraggable({
    id,
    disabled: disabled || !item || resizingItemId !== null,
    data: item,
  })

  const springX = useSpring(0, SPRING_OPTIONS)
  const springY = useSpring(0, SPRING_OPTIONS)

  const dropInfo = dropInfoMap[id]
  const prevDropInfoRef = useRef<typeof dropInfo>(undefined)

  useLayoutEffect(() => {
    if (!dropInfo || dropInfo === prevDropInfoRef.current || !item) {
      return
    }

    prevDropInfoRef.current = dropInfo
    const rect = getItemRect(item)
    springX.jump(dropInfo.left - rect.left)
    springY.jump(dropInfo.top - rect.top)
    springX.set(0)
    springY.set(0)
  }, [dropInfo, item, getItemRect, springX, springY])

  if (!item) {
    return null
  }

  const rect = getItemRect(item)
  const isActiveItem = activeItemId === id
  const isResizing = resizingItemId === id

  return (
    <motion.div
      ref={setNodeRef}
      initial={false}
      animate={{
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      }}
      transition={
        isResizing
          ? {
              left: RESIZE_SPRING,
              top: RESIZE_SPRING,
              width: RESIZE_SPRING,
              height: RESIZE_SPRING,
            }
          : {
              left: INSTANT,
              top: INSTANT,
              width: INSTANT,
              height: INSTANT,
            }
      }
      className={cn('group', className)}
      style={{
        position: 'absolute',
        touchAction: 'none',
        opacity: isActiveItem ? 0 : 1,
        x: springX,
        y: springY,
      }}
      {...attributes}
      {...listeners}
    >
      {children}

      <AnimatePresence>
        {!disabled && (
          <ResizeKnob
            onStart={(x, y) => onResizeStart(id, 'bottom-right', x, y)}
            onMove={onResizeMove}
            onEnd={onResizeEnd}
          />
        )}
      </AnimatePresence>
    </motion.div>
  )
}

export function DndGridItem({
  id,
  className,
  children,
  minW,
  minH,
  maxW,
  maxH,
}: DndGridItemProps) {
  const { isOverlay } = useDndGridContext()

  if (isOverlay) {
    // Inside DragOverlay: skip positioning and drag logic — just fill the overlay div.
    return <div className={cn('size-full', className)}>{children}</div>
  }

  return (
    <DndGridItemDraggable
      id={id}
      className={className}
      minW={minW}
      minH={minH}
      maxW={maxW}
      maxH={maxH}
    >
      {children}
    </DndGridItemDraggable>
  )
}
