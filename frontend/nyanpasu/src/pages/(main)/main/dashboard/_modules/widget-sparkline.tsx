import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { filesize } from 'filesize'
import { ComponentProps, ComponentType, ReactNode } from 'react'
import { Card, CardContent } from '@/components/ui/card'
import { Sparkline } from '@/components/ui/sparkline'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import {
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { WidgetComponentProps } from './consts'
import { useWidgetConfig } from './provider'
import { WidgetId } from './widget-config'
import WidgetItem, { WidgetItemProps } from './widget-item'

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

function SparklineCardTitle({
  icon: Icon,
  className,
  children,
  ...props
}: ComponentProps<'div'> & {
  icon: ComponentType<{
    className?: string
  }>
}) {
  return (
    <div
      className={cn('flex items-center gap-2', className)}
      data-slot="widget-sparkline-card-title"
      {...props}
    >
      <Icon className="size-5 shrink-0" />

      <TextMarquee className="font-bold">{children}</TextMarquee>
    </div>
  )
}

function SparklineCardContent({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn('text-2xl font-bold text-nowrap text-shadow-md', className)}
      data-slot="widget-sparkline-card-content"
      {...props}
    />
  )
}

function SparklineCardBottom({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'text-shadow-background h-5 text-sm text-nowrap text-shadow-xs',
        className,
      )}
      data-slot="widget-sparkline-card-bottom"
      {...props}
    />
  )
}

// Samples arrive several times a second. Only these leaves read them, so a
// sample re-renders the chart and the figures, not the widget around them.

const chartClass = 'absolute inset-0 z-0'

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

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficDown}
      chart={config.showChart && <TrafficChart direction="down" />}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={ArrowDownwardRounded}>
        {m.dashboard_widget_traffic_download()}
      </SparklineCardTitle>

      <SparklineCardContent>
        <TrafficRate direction="down" unit={config.unit} />
      </SparklineCardContent>

      {config.showTotal && (
        <SparklineCardBottom>
          <TrafficTotal field="downloadTotal" />
        </SparklineCardBottom>
      )}
    </SparklineCard>
  )
}

export function TrafficUpWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.TrafficUp)

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficUp}
      chart={config.showChart && <TrafficChart direction="up" />}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={ArrowUpwardRounded}>
        {m.dashboard_widget_traffic_upload()}
      </SparklineCardTitle>

      <SparklineCardContent>
        <TrafficRate direction="up" unit={config.unit} />
      </SparklineCardContent>

      {config.showTotal && (
        <SparklineCardBottom>
          <TrafficTotal field="uploadTotal" />
        </SparklineCardBottom>
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

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Connections}
      chart={config.showChart && <ConnectionsChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_connections()}
      </SparklineCardTitle>

      <SparklineCardContent>
        <ConnectionsCount />
      </SparklineCardContent>

      <SparklineCardBottom />
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

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Memory}
      chart={config.showChart && <MemoryChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={MemoryOutlineRounded}>
        {m.dashboard_widget_memory()}
      </SparklineCardTitle>

      <SparklineCardContent>
        <MemoryInUse />
      </SparklineCardContent>

      <SparklineCardBottom />
    </SparklineCard>
  )
}
