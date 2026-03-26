export type ResizeHandle =
  | 'top'
  | 'top-right'
  | 'right'
  | 'bottom-right'
  | 'bottom'
  | 'bottom-left'
  | 'left'
  | 'top-left'

export type DndGridItemType<T = string> = {
  id: T
  /** Column start index (0-indexed) */
  x: number
  /** Row start index (0-indexed) */
  y: number
  /** Width in grid cells */
  w: number
  /** Height in grid cells */
  h: number
}

export type GridItemConstraints = {
  /** Minimum width in grid cells (default: 1) */
  minW?: number
  /** Minimum height in grid cells (default: 1) */
  minH?: number
  /** Maximum width in grid cells */
  maxW?: number
  /** Maximum height in grid cells */
  maxH?: number
}

export interface GridSize {
  cols: number
  rows: number
}

export interface GridLayout extends GridSize {
  cellW: number
  cellH: number
}

export interface ItemRect {
  left: number
  top: number
  width: number
  height: number
}
