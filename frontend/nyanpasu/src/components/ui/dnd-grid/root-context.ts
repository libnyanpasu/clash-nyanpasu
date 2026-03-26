import { createContext, useContext } from 'react'
import type { DragEndEvent, DragMoveEvent, DragStartEvent } from '@dnd-kit/core'

export type GridRegistration = {
  itemIds: string[]
  dragIdPrefix: string
  sourceOnly: boolean
  handleDragStart: (e: DragStartEvent) => void
  handleDragMove: (e: DragMoveEvent) => void
  handleDragEnd: (e: DragEndEvent) => void
  handleDragCancel: () => void
  getCellSize: () => { cellW: number; cellH: number; gap: number }
  onSourceDrop?: (itemId: string) => void
  onSourceDragStart?: () => void
}

export type ActiveDrag = {
  itemId: string
  dims: { width: number; height: number }
}

export type DndGridRootContextValue = {
  registerGrid: (gridId: string, reg: GridRegistration) => void
  unregisterGrid: (gridId: string) => void
  activeDrag: ActiveDrag | null
}

export const DndGridRootContext = createContext<DndGridRootContextValue | null>(
  null,
)

export function useDndGridRoot() {
  return useContext(DndGridRootContext)
}
