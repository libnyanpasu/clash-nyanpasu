import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import type { ReactNode } from 'react'
import { Sparkline } from '@nyanpasu/ui/sparkline'
import { m } from '@/paraglide/messages'
import { formatBytes } from '@/utils/format-bytes'
import {
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@nyanpasu/query'
import {
  defineWidget,
  useWidgetConfig,
  useWidgetConfigActions,
  Widget,
  WidgetContent,
  WidgetHeader,
  WidgetMeta,
  WidgetMetric,
  WidgetTitle,
} from './core'
import { DEFAULT_WIDGET_CONFIGS, WidgetId } from './widget-config'
import {
  ConfigChoice,
  ConfigStepper,
  ConfigToggle,
} from './widget-config-fields'
import { ConfigGroup } from './widget-config-row'

const padData = (data: (number | undefined)[] = [], max: number) =>
  Array(Math.max(0, max - data.length))
    .fill(0)
    .concat(data.slice(-max))

// Samples arrive several times a second. Only these leaves read them, so a
// sample re-renders the chart and the figures, not the widget around them.

const chartClass = 'absolute inset-0 z-0'

const PREVIEW_DOWN_TRAFFIC = [
  180_000, 240_000, 210_000, 420_000, 360_000, 510_000, 460_000, 720_000,
]
const PREVIEW_UP_TRAFFIC = [
  45_000, 60_000, 52_000, 90_000, 78_000, 120_000, 135_000, 180_000,
]
const PREVIEW_CONNECTIONS = [4, 6, 5, 8, 7, 10, 9, 12]
const PREVIEW_MEMORY = [420, 448, 462, 456, 492, 510, 536, 560].map(
  (mebibytes) => mebibytes * 1024 * 1024,
)

function SparklineView({
  chart,
  children,
}: {
  chart?: ReactNode
  children: ReactNode
}) {
  return (
    <Widget
      scaling="proportional"
      data-widget-kind="sparkline"
      data-slot="widget-sparkline-card"
    >
      {chart}

      <WidgetHeader>
        <WidgetTitle />
      </WidgetHeader>

      <WidgetContent layout="spread">{children}</WidgetContent>
    </Widget>
  )
}

function TrafficChart({ direction }: { direction: 'up' | 'down' }) {
  const { data: clashTraffic } = useClashTraffic()

  return (
    <Sparkline
      data={padData(
        clashTraffic?.map((item) => item[direction]),
        MAX_TRAFFIC_HISTORY,
      )}
      className={chartClass}
    />
  )
}

function TrafficRate({
  direction,
  unit,
}: {
  direction: 'up' | 'down'
  unit: 'bytes' | 'bits'
}) {
  const { data: clashTraffic } = useClashTraffic()

  return (
    <>
      {formatBytes(clashTraffic?.at(-1)?.[direction] ?? 0, {
        standard: unit === 'bits' ? 'si' : 'iec',
        bits: unit === 'bits',
      })}
      /s
    </>
  )
}

function TrafficTotal({ field }: { field: 'downloadTotal' | 'uploadTotal' }) {
  const { data: clashConnections } = useClashConnections()

  const total = clashConnections?.at(-1)?.[field]

  return (
    total !== undefined &&
    m.dashboard_widget_traffic_total({
      value: formatBytes(total),
    })
  )
}

function TrafficLive({ direction }: { direction: 'up' | 'down' }) {
  const config = useWidgetConfig(
    direction === 'down' ? WidgetId.TrafficDown : WidgetId.TrafficUp,
  )

  return (
    <SparklineView
      chart={config.showChart && <TrafficChart direction={direction} />}
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>
          <TrafficRate direction={direction} unit={config.unit} />
        </WidgetMetric>
      </div>

      {config.showTotal && (
        <WidgetMeta>
          <TrafficTotal
            field={direction === 'down' ? 'downloadTotal' : 'uploadTotal'}
          />
        </WidgetMeta>
      )}
    </SparklineView>
  )
}

function TrafficPreview({ direction }: { direction: 'up' | 'down' }) {
  const config = useWidgetConfig(
    direction === 'down' ? WidgetId.TrafficDown : WidgetId.TrafficUp,
  )
  const latest = direction === 'down' ? 720_000 : 180_000
  const total = direction === 'down' ? 86_400_000 : 12_800_000
  const chart = direction === 'down' ? PREVIEW_DOWN_TRAFFIC : PREVIEW_UP_TRAFFIC

  return (
    <SparklineView
      chart={
        config.showChart && <Sparkline data={chart} className={chartClass} />
      }
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>
          {formatBytes(latest, {
            standard: config.unit === 'bits' ? 'si' : 'iec',
            bits: config.unit === 'bits',
          })}
          /s
        </WidgetMetric>
      </div>

      {config.showTotal && (
        <WidgetMeta>
          {m.dashboard_widget_traffic_total({
            value: formatBytes(total),
          })}
        </WidgetMeta>
      )}
    </SparklineView>
  )
}

function ConnectionsLive() {
  const config = useWidgetConfig(WidgetId.Connections)
  const { data: clashConnections } = useClashConnections()

  return (
    <SparklineView
      chart={
        config.showChart && (
          <Sparkline
            data={padData(
              clashConnections?.map((item) => item.connectionCount),
              config.samples,
            )}
            className={chartClass}
          />
        )
      }
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>
          {clashConnections?.at(-1)?.connectionCount ?? 0}
        </WidgetMetric>
      </div>
    </SparklineView>
  )
}

function ConnectionsPreview() {
  const config = useWidgetConfig(WidgetId.Connections)

  return (
    <SparklineView
      chart={
        config.showChart && (
          <Sparkline data={PREVIEW_CONNECTIONS} className={chartClass} />
        )
      }
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>12</WidgetMetric>
      </div>
    </SparklineView>
  )
}

function MemoryLive() {
  const config = useWidgetConfig(WidgetId.Memory)
  const { data: clashMemory } = useClashMemory()

  return (
    <SparklineView
      chart={
        config.showChart && (
          <Sparkline
            data={padData(
              clashMemory?.map((item) => item.inuse),
              config.samples,
            )}
            className={chartClass}
          />
        )
      }
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>
          {formatBytes(clashMemory?.at(-1)?.inuse ?? 0)}
        </WidgetMetric>
      </div>
    </SparklineView>
  )
}

function MemoryPreview() {
  const config = useWidgetConfig(WidgetId.Memory)

  return (
    <SparklineView
      chart={
        config.showChart && (
          <Sparkline data={PREVIEW_MEMORY} className={chartClass} />
        )
      }
    >
      <div className="flex min-h-0 flex-1 items-center">
        <WidgetMetric>{formatBytes(PREVIEW_MEMORY.at(-1)!)}</WidgetMetric>
      </div>
    </SparklineView>
  )
}

function TrafficConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions()

  if (
    config.type !== WidgetId.TrafficDown &&
    config.type !== WidgetId.TrafficUp
  ) {
    return null
  }

  return (
    <ConfigGroup>
      <ConfigToggle
        label={m.dashboard_widget_sparkline_config_chart()}
        checked={config.showChart}
        disabled={disabled}
        onChange={(showChart) => updateConfig({ showChart })}
      />
      <ConfigToggle
        label={m.dashboard_widget_traffic_config_total()}
        checked={config.showTotal}
        disabled={disabled}
        onChange={(showTotal) => updateConfig({ showTotal })}
      />
      <ConfigChoice
        label={m.dashboard_widget_traffic_config_speed_unit()}
        value={config.unit}
        disabled={disabled}
        options={[
          { value: 'bytes', label: 'B/s' },
          { value: 'bits', label: 'bit/s' },
        ]}
        onChange={(unit) => updateConfig({ unit })}
      />
    </ConfigGroup>
  )
}

function HistoryConfigForm() {
  const { config, disabled, updateConfig } = useWidgetConfigActions()

  if (config.type !== WidgetId.Memory && config.type !== WidgetId.Connections) {
    return null
  }

  return (
    <ConfigGroup>
      <ConfigToggle
        label={m.dashboard_widget_sparkline_config_chart()}
        checked={config.showChart}
        disabled={disabled}
        onChange={(showChart) => updateConfig({ showChart })}
      />
      <ConfigStepper
        label={m.dashboard_widget_sparkline_config_samples()}
        value={config.samples}
        range={[2, 32]}
        disabled={disabled || !config.showChart}
        onChange={(samples) => updateConfig({ samples })}
      />
    </ConfigGroup>
  )
}

export const trafficDownWidget = defineWidget({
  id: WidgetId.TrafficDown,
  category: 'realtime',
  title: m.dashboard_widget_traffic_download,
  icon: ArrowDownwardRounded,
  size: { minW: 2, minH: 2, defaultW: 2, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficDown],
  Component: () => <TrafficLive direction="down" />,
  Preview: () => <TrafficPreview direction="down" />,
  ConfigForm: TrafficConfigForm,
})

export const trafficUpWidget = defineWidget({
  id: WidgetId.TrafficUp,
  category: 'realtime',
  title: m.dashboard_widget_traffic_upload,
  icon: ArrowUpwardRounded,
  size: { minW: 2, minH: 2, defaultW: 2, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.TrafficUp],
  Component: () => <TrafficLive direction="up" />,
  Preview: () => <TrafficPreview direction="up" />,
  ConfigForm: TrafficConfigForm,
})

export const connectionsWidget = defineWidget({
  id: WidgetId.Connections,
  category: 'realtime',
  title: m.dashboard_widget_connections,
  icon: SettingsEthernetRounded,
  size: { minW: 2, minH: 2, defaultW: 2, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.Connections],
  Component: ConnectionsLive,
  Preview: ConnectionsPreview,
  ConfigForm: HistoryConfigForm,
})

export const memoryWidget = defineWidget({
  id: WidgetId.Memory,
  category: 'realtime',
  title: m.dashboard_widget_memory,
  icon: MemoryOutlineRounded,
  size: { minW: 2, minH: 2, defaultW: 2, defaultH: 2 },
  defaultConfig: DEFAULT_WIDGET_CONFIGS[WidgetId.Memory],
  Component: MemoryLive,
  Preview: MemoryPreview,
  ConfigForm: HistoryConfigForm,
})
