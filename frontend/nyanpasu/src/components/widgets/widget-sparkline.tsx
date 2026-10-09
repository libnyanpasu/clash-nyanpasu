import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { filesize } from 'filesize'
import { ComponentProps, ReactNode } from 'react'
import { Card, CardContent } from '@nyanpasu/ui/card'
import { useDndGridContext } from '@nyanpasu/ui/dnd-grid'
import { Sparkline } from '@nyanpasu/ui/sparkline'
import { m } from '@/paraglide/messages'
import {
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@nyanpasu/query'
import { cn } from '@nyanpasu/utils'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem, { WidgetItemProps } from './widget-item'
import { WidgetMeta, WidgetMetric, WidgetTitle } from './widget-ui'

const padData = (data: (number | undefined)[] = [], max: number) =>
  Array(Math.max(0, max - data.length))
    .fill(0)
    .concat(data.slice(-max))

function SparklineCard({
  id,
  minH = 2,
  minW = 2,
  maxW,
  maxH,
  chart,
  className,
  children,
  onCloseClick,
  widgetType,
  ...props
}: ComponentProps<typeof Card> & {
  chart: ReactNode
} & WidgetItemProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={widgetType}
      minH={minH}
      minW={minW}
      maxW={maxW}
      maxH={maxH}
      onCloseClick={onCloseClick}
    >
      <Card
        className={cn('relative isolate size-full', className)}
        data-slot="widget-sparkline-card"
        {...props}
      >
        {chart}

        <CardContent
          className="relative z-10 flex size-full flex-col justify-between"
          data-slot="widget-sparkline-card-content"
        >
          {children}
        </CardContent>
      </Card>
    </WidgetItem>
  )
}

// Samples arrive several times a second. Only these leaves read them, so a
// sample re-renders the chart and the figures, not the widget around them.

const chartClass = 'absolute inset-0 z-0'
const previewDownloadTraffic = [
  180_000, 240_000, 210_000, 420_000, 360_000, 510_000, 460_000, 720_000,
]
const previewUploadTraffic = [
  45_000, 60_000, 52_000, 90_000, 78_000, 120_000, 135_000, 180_000,
]
const previewConnections = [4, 6, 5, 8, 7, 10, 9, 12]
const previewMemory = [420, 448, 462, 456, 492, 510, 536, 560].map(
  (mebibytes) => mebibytes * 1024 * 1024,
)

function TrafficPreview({
  direction,
  unit,
  showTotal,
}: {
  direction: 'up' | 'down'
  unit: 'bytes' | 'bits'
  showTotal: boolean
}) {
  const latest = direction === 'down' ? 720_000 : 180_000
  const total = direction === 'down' ? 86_400_000 : 12_800_000

  return (
    <>
      <WidgetTitle
        icon={direction === 'down' ? ArrowDownwardRounded : ArrowUpwardRounded}
      >
        {direction === 'down'
          ? m.dashboard_widget_traffic_download()
          : m.dashboard_widget_traffic_upload()}
      </WidgetTitle>
      <WidgetMetric>
        {filesize(latest, {
          standard: unit === 'bits' ? 'si' : 'iec',
          bits: unit === 'bits',
        })}
        /s
      </WidgetMetric>
      {showTotal && (
        <WidgetMeta>
          {m.dashboard_widget_traffic_total({
            value: filesize(total, { standard: 'iec' }),
          })}
        </WidgetMeta>
      )}
    </>
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
      {filesize(clashTraffic?.at(-1)?.[direction] ?? 0, {
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
      value: filesize(total, { standard: 'iec' }),
    })
  )
}

export function TrafficDownWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.TrafficDown)
  const { sourceOnly, isOverlay } = useDndGridContext()

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.TrafficDown}
        chart={
          config.showChart && (
            <Sparkline data={previewDownloadTraffic} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <TrafficPreview
          direction="down"
          unit={config.unit}
          showTotal={config.showTotal}
        />
      </SparklineCard>
    )
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficDown}
      chart={config.showChart && <TrafficChart direction="down" />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle icon={ArrowDownwardRounded}>
        {m.dashboard_widget_traffic_download()}
      </WidgetTitle>

      <WidgetMetric>
        <TrafficRate direction="down" unit={config.unit} />
      </WidgetMetric>

      {config.showTotal && (
        <WidgetMeta>
          <TrafficTotal field="downloadTotal" />
        </WidgetMeta>
      )}
    </SparklineCard>
  )
}

export function TrafficUpWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.TrafficUp)
  const { sourceOnly, isOverlay } = useDndGridContext()

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.TrafficUp}
        chart={
          config.showChart && (
            <Sparkline data={previewUploadTraffic} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <TrafficPreview
          direction="up"
          unit={config.unit}
          showTotal={config.showTotal}
        />
      </SparklineCard>
    )
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficUp}
      chart={config.showChart && <TrafficChart direction="up" />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle icon={ArrowUpwardRounded}>
        {m.dashboard_widget_traffic_upload()}
      </WidgetTitle>

      <WidgetMetric>
        <TrafficRate direction="up" unit={config.unit} />
      </WidgetMetric>

      {config.showTotal && (
        <WidgetMeta>
          <TrafficTotal field="uploadTotal" />
        </WidgetMeta>
      )}
    </SparklineCard>
  )
}

function ConnectionsChart({ samples }: { samples: number }) {
  const { data: clashConnections } = useClashConnections()

  return (
    <Sparkline
      data={padData(
        clashConnections?.map((item) => item.connectionCount),
        samples,
      )}
      className={chartClass}
    />
  )
}

function ConnectionsCount() {
  const { data: clashConnections } = useClashConnections()

  return <>{clashConnections?.at(-1)?.connectionCount ?? 0}</>
}

export function ConnectionsWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.Connections)
  const { sourceOnly, isOverlay } = useDndGridContext()

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.Connections}
        chart={
          config.showChart && (
            <Sparkline data={previewConnections} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <WidgetTitle icon={SettingsEthernetRounded}>
          {m.dashboard_widget_connections()}
        </WidgetTitle>
        <WidgetMetric>12</WidgetMetric>
        <WidgetMeta />
      </SparklineCard>
    )
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Connections}
      chart={config.showChart && <ConnectionsChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_connections()}
      </WidgetTitle>

      <WidgetMetric>
        <ConnectionsCount />
      </WidgetMetric>

      <WidgetMeta />
    </SparklineCard>
  )
}

function MemoryChart({ samples }: { samples: number }) {
  const { data: clashMemory } = useClashMemory()

  return (
    <Sparkline
      data={padData(
        clashMemory?.map((item) => item.inuse),
        samples,
      )}
      className={chartClass}
    />
  )
}

function MemoryInUse() {
  const { data: clashMemory } = useClashMemory()

  return <>{filesize(clashMemory?.at(-1)?.inuse ?? 0, { standard: 'iec' })}</>
}

export function MemoryWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.Memory)
  const { sourceOnly, isOverlay } = useDndGridContext()

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.Memory}
        chart={
          config.showChart && (
            <Sparkline data={previewMemory} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <WidgetTitle icon={MemoryOutlineRounded}>
          {m.dashboard_widget_memory()}
        </WidgetTitle>
        <WidgetMetric>
          {filesize(previewMemory.at(-1)!, { standard: 'iec' })}
        </WidgetMetric>
        <WidgetMeta />
      </SparklineCard>
    )
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Memory}
      chart={config.showChart && <MemoryChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle icon={MemoryOutlineRounded}>
        {m.dashboard_widget_memory()}
      </WidgetTitle>

      <WidgetMetric>
        <MemoryInUse />
      </WidgetMetric>

      <WidgetMeta />
    </SparklineCard>
  )
}
