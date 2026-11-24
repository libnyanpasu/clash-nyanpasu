import type { WidgetId } from '../widget-config'
import type { WidgetCategoryId, WidgetDefinition } from './define-widget'

export const WIDGET_CATEGORY_IDS = [
  'realtime',
  'analytics',
  'controls',
  'subscription',
  'network',
] as const satisfies readonly WidgetCategoryId[]

const registry = new Map<WidgetId, WidgetDefinition>()

export const WIDGET_REGISTRY: ReadonlyMap<WidgetId, WidgetDefinition> = registry

/** Adds a widget definition to the shared registry. */
export function registerWidget(definition: WidgetDefinition): void {
  registry.set(definition.id, definition)
}

/** Adds the definitions imported by the widget item modules. */
export function registerWidgets(
  definitions: readonly WidgetDefinition[],
): void {
  for (const definition of definitions) {
    registerWidget(definition)
  }
}

/** Looks up a registered widget, or reports the missing registration early. */
export function getWidgetDefinition<T extends WidgetId>(
  type: T,
): WidgetDefinition<T> {
  const definition = registry.get(type)

  if (!definition) {
    throw new Error(
      `Widget "${type}" is not registered. Call registerWidgets() with its definition first.`,
    )
  }

  return definition as WidgetDefinition<T>
}

/** The registered definitions, in registration order. */
export function getWidgetDefinitions(): readonly WidgetDefinition[] {
  return [...registry.values()]
}
