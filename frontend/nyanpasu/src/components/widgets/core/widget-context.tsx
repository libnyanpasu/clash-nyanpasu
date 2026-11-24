import { createContext, use, useMemo, type PropsWithChildren } from 'react'
import type { WidgetConfig, WidgetConfigs, WidgetId } from '../widget-config'
import type { WidgetDefinition } from './define-widget'

export type WidgetRenderMode = 'live' | 'preview' | 'overlay'

export interface WidgetSpan {
  w: number
  h: number
}

export interface WidgetInstanceContextValue<T extends WidgetId = WidgetId> {
  id: string
  type: T
  mode: WidgetRenderMode
  interactive: boolean
  isEditing: boolean
  span: WidgetSpan
  definition: WidgetDefinition<T>
  config: WidgetConfigs[T]
  configDisabled: boolean
  updateConfig: (patch: Partial<WidgetConfigs[T]>) => void
  resetConfig: () => void
  enterEditMode: () => void
}

export const WidgetInstanceContext =
  createContext<WidgetInstanceContextValue | null>(null)

export function useWidgetContext<
  T extends WidgetId = WidgetId,
>(): WidgetInstanceContextValue<T> {
  const context = use(WidgetInstanceContext)

  if (!context) {
    throw new Error('useWidgetContext must be used within a widget host')
  }

  return context as unknown as WidgetInstanceContextValue<T>
}

/** The instance context under the name used by the widget primitives. */
export function useWidgetInstance<
  T extends WidgetId = WidgetId,
>(): WidgetInstanceContextValue<T> {
  return useWidgetContext<T>()
}

/** Reads the widget's typed config; previews always show the definition default. */
export function useWidgetConfig<T extends WidgetId = WidgetId>(
  type?: T,
): WidgetConfigs[T] {
  const context = useWidgetContext<T>()

  if (type !== undefined && context.type !== type) {
    throw new Error(
      `useWidgetConfig expected "${type}" but the host renders "${context.type}"`,
    )
  }

  return context.mode === 'preview'
    ? context.definition.defaultConfig
    : context.config
}

/** The config and its mutation controls for a widget's config form. */
export function useWidgetConfigActions<T extends WidgetId = WidgetId>(
  type?: T,
) {
  const context = useWidgetContext<T>()

  if (type !== undefined && context.type !== type) {
    throw new Error(
      `useWidgetConfigActions expected "${type}" but the host renders "${context.type}"`,
    )
  }

  return {
    config:
      context.mode === 'preview'
        ? context.definition.defaultConfig
        : context.config,
    disabled: context.configDisabled || context.mode !== 'live',
    updateConfig: context.updateConfig,
    resetConfig: context.resetConfig,
  }
}

export function useWidgetInteractive(): boolean {
  return useWidgetContext().interactive
}

interface WidgetInstanceProviderProps<
  T extends WidgetId = WidgetId,
> extends PropsWithChildren {
  id: string
  type: T
  mode: WidgetRenderMode
  interactive: boolean
  isEditing: boolean
  span: WidgetSpan
  definition: WidgetDefinition<T>
  config: WidgetConfigs[T]
  configDisabled?: boolean
  updateConfig?: (patch: Partial<WidgetConfigs[T]>) => void
  resetConfig?: () => void
  enterEditMode?: () => void
}

const noop = () => {}

export function WidgetInstanceProvider<T extends WidgetId>({
  id,
  type,
  mode,
  interactive,
  isEditing,
  span,
  definition,
  config,
  configDisabled = false,
  updateConfig = noop,
  resetConfig = noop,
  enterEditMode = noop,
  children,
}: WidgetInstanceProviderProps<T>) {
  const value = useMemo<WidgetInstanceContextValue>(
    () => ({
      id,
      type,
      mode,
      interactive,
      isEditing,
      span: { w: span.w, h: span.h },
      definition,
      config,
      configDisabled,
      updateConfig: updateConfig as (patch: Partial<WidgetConfig>) => void,
      resetConfig,
      enterEditMode,
    }),
    [
      id,
      type,
      mode,
      interactive,
      isEditing,
      span.w,
      span.h,
      definition,
      config,
      configDisabled,
      updateConfig,
      resetConfig,
      enterEditMode,
    ],
  )

  return (
    <WidgetInstanceContext.Provider value={value}>
      {children}
    </WidgetInstanceContext.Provider>
  )
}
