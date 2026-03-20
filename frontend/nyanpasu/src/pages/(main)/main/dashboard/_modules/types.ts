import type { ComponentType } from 'react'
import type { ResponsiveLayouts } from 'react-grid-layout'

export interface WidgetDef {
  id: string
  title: string
  Component: ComponentType
  minW: number
  minH: number
}

export interface DashboardState {
  layouts: ResponsiveLayouts
  activeWidgets: string[]
}
