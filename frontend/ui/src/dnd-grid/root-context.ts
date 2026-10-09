import { createContext, useContext } from 'react'
import type { DragEndEvent, DragMoveEvent, DragStartEvent } from '@dnd-kit/core'
import type { GridPosition } from './types'

export type GridRegistration = {
  itemIds: string[]
  dragIdPrefix: string
  sourceOnly: boolean
  handleDragStart: (e: DragStartEvent) => void
  handleDragMove: (e: DragMoveEvent) => void
  handleDragEnd: (e: DragEndEvent) => void
  handleDragCancel: () => void
  getCellSize: () => { cellW: number; cellH: number; gap: number }
  getDropPosition: (clientX: number, clientY: number) => GridPosition | null
  onExternalDrop?: (itemId: string, position: GridPosition) => void
  onSourceDragStart?: () => void
}

export type ActiveDrag = {
  itemId: string
  dragIdPrefix: string
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
