import type { ReactNode } from 'react'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import type { DashboardItem, WidgetComponentProps } from './consts'
import { WidgetGridHost, WidgetOverlayHost, WidgetPreview } from './core'
import { useDashboardContext } from './provider'
import { getWidgetDefinition, WIDGET_LIST } from './registry'
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WidgetConfig,
  WidgetId,
} from './widget-config'
import { WidgetConfigControl } from './widget-config-control'

/**
 * Temporary bridge for the legacy `RENDER_MAP` call sites while the dashboard
 * and library are migrated in the next phases. It is the single place that
 * chooses live, preview, or overlay mode; the widgets themselves no longer do.
 */
function WidgetRenderer({
  type,
  id,
  onCloseClick,
}: WidgetComponentProps & { type: WidgetId }) {
  const { sourceOnly, isOverlay, disabled, displayItems } = useDndGridContext()
  const { configs, saveConfig, setIsEditing, configLoading, configReadError } =
    useDashboardContext()
  const definition = getWidgetDefinition(type)
  const config = getWidgetConfig(configs, id, type)

  if (sourceOnly) {
    return <WidgetPreview definition={definition} />
  }

  if (isOverlay) {
    return (
      <WidgetOverlayHost type={type} definition={definition} config={config} />
    )
  }

  const found = displayItems.find((entry) => entry.id === id)
  const item: DashboardItem = found
    ? { ...found, type: (found as Partial<DashboardItem>).type ?? type }
    : {
        id,
        type,
        x: 0,
        y: 0,
        w: definition.size.defaultW,
        h: definition.size.defaultH,
      }

  return (
    <WidgetGridHost
      item={item}
      definition={definition}
      config={config}
      configDisabled={configLoading || configReadError}
      interactive={disabled}
      isEditing={!disabled}
      updateConfig={(patch) =>
        saveConfig(id, { ...config, ...patch } as WidgetConfig)
      }
      resetConfig={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
      enterEditMode={() => setIsEditing(true)}
      onRemove={onCloseClick}
    >
      <WidgetConfigControl id={id} type={type} isActive={!disabled} />
    </WidgetGridHost>
  )
}

const legacyRenderer = (type: WidgetId) => {
  function LegacyWidget(props: WidgetComponentProps) {
    return <WidgetRenderer type={type} {...props} />
  }

  LegacyWidget.displayName = `LegacyWidget(${type})`
  return LegacyWidget
}

export const RENDER_MAP: Record<
  WidgetId,
  (props: WidgetComponentProps) => ReactNode
> = {
  [WidgetId.TrafficDown]: legacyRenderer(WidgetId.TrafficDown),
  [WidgetId.TrafficUp]: legacyRenderer(WidgetId.TrafficUp),
  [WidgetId.Connections]: legacyRenderer(WidgetId.Connections),
  [WidgetId.Memory]: legacyRenderer(WidgetId.Memory),
  [WidgetId.ProxyShortcuts]: legacyRenderer(WidgetId.ProxyShortcuts),
  [WidgetId.CoreShortcuts]: legacyRenderer(WidgetId.CoreShortcuts),
  [WidgetId.SubscriptionQuota]: legacyRenderer(WidgetId.SubscriptionQuota),
  [WidgetId.SubscriptionSchedule]: legacyRenderer(
    WidgetId.SubscriptionSchedule,
  ),
  [WidgetId.ProxyMode]: legacyRenderer(WidgetId.ProxyMode),
  [WidgetId.RecentTraffic]: legacyRenderer(WidgetId.RecentTraffic),
  [WidgetId.OriginTraffic]: legacyRenderer(WidgetId.OriginTraffic),
  [WidgetId.ExitTraffic]: legacyRenderer(WidgetId.ExitTraffic),
  [WidgetId.TargetTraffic]: legacyRenderer(WidgetId.TargetTraffic),
  [WidgetId.RuleTraffic]: legacyRenderer(WidgetId.RuleTraffic),
  [WidgetId.ActiveConnections]: legacyRenderer(WidgetId.ActiveConnections),
  [WidgetId.ProviderUpdates]: legacyRenderer(WidgetId.ProviderUpdates),
  [WidgetId.NetworkInfo]: legacyRenderer(WidgetId.NetworkInfo),
}

export const WIDGET_MIN_SIZE_MAP = Object.fromEntries(
  WIDGET_LIST.map((definition) => [
    definition.id,
    { minW: definition.size.minW, minH: definition.size.minH },
  ]),
) as Record<WidgetId, { minW: number; minH: number }>

export const WIDGET_RECOMMENDED_SIZE_MAP = Object.fromEntries(
  WIDGET_LIST.map((definition) => [
    definition.id,
    { w: definition.size.defaultW, h: definition.size.defaultH },
  ]),
) as Record<WidgetId, { w: number; h: number }>
