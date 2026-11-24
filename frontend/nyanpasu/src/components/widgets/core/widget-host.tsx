import type { ReactNode } from 'react'
import { DndGridItem, type DndGridItemType } from '@nyanpasu/ui/dnd-grid'
import { cn } from '@nyanpasu/utils'
import type { WidgetConfig, WidgetId } from '../widget-config'
import type { WidgetDefinition } from './define-widget'
import { getWidgetDefinition } from './registry'
import {
  WidgetInstanceProvider,
  type WidgetRenderMode,
  type WidgetSpan,
} from './widget-context'

function resolveDefinition(
  definition: WidgetDefinition | undefined,
  type: WidgetId | undefined,
): WidgetDefinition {
  if (definition) {
    return definition
  }

  if (type !== undefined) {
    return getWidgetDefinition(type)
  }

  throw new Error('A widget host requires a widget definition or type')
}

/**
 * Splits the data layer from the presentation layer before mounting any
 * widget. Preview and overlay modes never mount `definition.Component`, so
 * they cannot issue queries.
 */
function WidgetModeRenderer({
  definition,
  mode,
}: {
  definition: WidgetDefinition
  mode: WidgetRenderMode
}) {
  const RenderedWidget =
    mode === 'live' ? definition.Component : definition.Preview

  return <RenderedWidget />
}

export type WidgetGridHostProps = {
  item: DndGridItemType<string> & { type: WidgetId }
  definition?: WidgetDefinition
  config?: WidgetConfig
  configDisabled?: boolean
  interactive?: boolean
  isEditing?: boolean
  updateConfig?: (patch: Partial<WidgetConfig>) => void
  resetConfig?: () => void
  enterEditMode?: () => void
  className?: string
  /** Host chrome, such as remove and config controls, renders inside the instance context. */
  children?: ReactNode
}

/** Hosts one live widget inside a dashboard grid item. */
export function WidgetGridHost({
  item,
  definition,
  config,
  configDisabled,
  interactive,
  isEditing = false,
  updateConfig,
  resetConfig,
  enterEditMode,
  className,
  children,
}: WidgetGridHostProps) {
  const resolved = resolveDefinition(definition, item.type)

  return (
    <DndGridItem
      id={item.id}
      minW={resolved.size.minW}
      minH={resolved.size.minH}
      maxW={resolved.size.maxW}
      maxH={resolved.size.maxH}
      className={cn('relative', className)}
    >
      <WidgetInstanceProvider
        id={item.id}
        type={item.type}
        mode="live"
        interactive={interactive ?? !isEditing}
        isEditing={isEditing}
        span={{ w: item.w, h: item.h }}
        definition={resolved}
        config={config ?? resolved.defaultConfig}
        configDisabled={configDisabled ?? updateConfig === undefined}
        updateConfig={updateConfig}
        resetConfig={resetConfig}
        enterEditMode={enterEditMode}
      >
        <WidgetModeRenderer definition={resolved} mode="live" />

        {children}
      </WidgetInstanceProvider>
    </DndGridItem>
  )
}

export type WidgetOverlayHostProps = {
  type: WidgetId
  definition?: WidgetDefinition
  config?: WidgetConfig
  span?: WidgetSpan
}

/** Renders the drag-overlay copy of a widget without dashboard data queries. */
export function WidgetOverlayHost({
  type,
  definition,
  config,
  span,
}: WidgetOverlayHostProps) {
  const resolved = resolveDefinition(definition, type)

  return (
    <WidgetInstanceProvider
      id={`overlay:${resolved.id}`}
      type={type}
      mode="overlay"
      interactive={false}
      isEditing={false}
      span={span ?? { w: resolved.size.defaultW, h: resolved.size.defaultH }}
      definition={resolved}
      config={config ?? resolved.defaultConfig}
      configDisabled
    >
      <WidgetModeRenderer definition={resolved} mode="overlay" />
    </WidgetInstanceProvider>
  )
}

export type WidgetPreviewHostProps = {
  type?: WidgetId
  definition?: WidgetDefinition
}

/** Renders a widget's static preview with its default configuration. */
export function WidgetPreviewHost({
  type,
  definition,
}: WidgetPreviewHostProps) {
  const resolved = resolveDefinition(definition, type)

  return (
    <WidgetInstanceProvider
      id={`preview:${resolved.id}`}
      type={resolved.id}
      mode="preview"
      interactive={false}
      isEditing={false}
      span={{ w: resolved.size.defaultW, h: resolved.size.defaultH }}
      definition={resolved}
      config={resolved.defaultConfig}
      configDisabled
    >
      <WidgetModeRenderer definition={resolved} mode="preview" />
    </WidgetInstanceProvider>
  )
}
