import type { ComponentType } from 'react'
import type { Permission } from '@nyanpasu/rpc/types'
import type { WidgetConfigs, WidgetId } from '../widget-config'

/** The gallery groups a widget is listed under. */
export type WidgetCategoryId =
  'realtime' | 'analytics' | 'controls' | 'subscription' | 'network'

export interface WidgetSizeSpec {
  minW: number
  minH: number
  maxW?: number
  maxH?: number
  defaultW: number
  defaultH: number
}

export interface WidgetDefinition<T extends WidgetId = WidgetId> {
  id: T
  category: WidgetCategoryId
  title: () => string
  description: () => string
  icon: ComponentType<{ className?: string }>
  size: WidgetSizeSpec
  permission?: Permission
  defaultConfig: WidgetConfigs[T]
  /** Mounted only while the widget is live; queries and subscriptions are safe here. */
  Component: ComponentType
  /** Mounted for preview and overlay modes; presentation only, with no queries. */
  Preview: ComponentType
  /** Optional form rendered by the shared widget config menu. */
  ConfigForm?: ComponentType
}

export function defineWidget<T extends WidgetId>(
  definition: WidgetDefinition<T>,
): WidgetDefinition<T> {
  return definition
}
