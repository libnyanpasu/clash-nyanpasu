import { createContext, useContext, type RefObject } from 'react'
import type {
  DndGridItemType,
  GridItemConstraints,
  ItemRect,
  ResizeHandle,
} from './types'

const DndGridContext = createContext<{
  displayItems: DndGridItemType[]
  getItemRect: (item: DndGridItemType) => ItemRect
  dropInfoMap: Record<string, { left: number; top: number }>
  activeItemId: string | null
  resizingItemId: string | null
  disabled: boolean
  isOverlay: boolean
  constraintsMapRef: RefObject<Record<string, GridItemConstraints>> & {
    current: Record<string, GridItemConstraints>
  }
  onResizeStart: (
    id: string,
    handle: ResizeHandle,
    startX: number,
    startY: number,
  ) => void
  onResizeMove: (currentX: number, currentY: number) => void
  onResizeEnd: () => void
} | null>(null)

export const DndGridProvider = DndGridContext.Provider

export function useDndGridContext() {
  const ctx = useContext(DndGridContext)

  if (!ctx) {
    throw new Error('DndGridItem must be used inside DndGrid')
  }

  return ctx
}
