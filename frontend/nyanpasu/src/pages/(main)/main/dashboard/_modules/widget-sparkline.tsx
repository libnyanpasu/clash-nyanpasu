import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded'
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded'
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded'
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded'
import { filesize } from 'filesize'
import { ComponentProps, ComponentType } from 'react'
import { Card, CardContent } from '@/components/ui/card'
import { Sparkline } from '@/components/ui/sparkline'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import {
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

const padData = (data: (number | undefined)[] = [], max: number) =>
  Array(Math.max(0, max - data.length))
    .fill(0)
    .concat(data.slice(-max))

function SparklineCard({
  data,
  className,
  children,
  ...props
}: ComponentProps<typeof Card> & {
  data: number[]
}) {
  return (
    <Card
      className={cn('relative isolate size-full', className)}
      data-slot="widget-sparkline-card"
      {...props}
    >
      <Sparkline data={data} className="absolute inset-0 z-0" />

      <CardContent
        className="relative z-10 flex size-full flex-col justify-between"
        data-slot="widget-sparkline-card-content"
      >
        {children}
      </CardContent>
    </Card>
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
      className={cn('text-2xl font-bold text-shadow-md', className)}
      data-slot="widget-sparkline-card-content"
      {...props}
    />
  )
}

function SparklineCardBottom({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'text-shadow-background h-5 text-sm text-shadow-xs',
        className,
      )}
      data-slot="widget-sparkline-card-bottom"
      {...props}
    />
  )
}

export function TrafficDownWidget() {
  const { data: clashTraffic } = useClashTraffic()

  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const total = clashConnections?.at(-1)?.downloadTotal

  return (
    <SparklineCard
      data={padData(
        clashTraffic?.map((item) => item.down),
        MAX_TRAFFIC_HISTORY,
      )}
    >
      <SparklineCardTitle icon={ArrowDownwardRounded}>
        {m.dashboard_widget_traffic_download()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashTraffic?.at(-1)?.down ?? 0)}/s
      </SparklineCardContent>

      <SparklineCardBottom>
        {total !== undefined &&
          m.dashboard_widget_traffic_total({
            value: filesize(total),
          })}
      </SparklineCardBottom>
    </SparklineCard>
  )
}

export function TrafficUpWidget() {
  const { data: clashTraffic } = useClashTraffic()

  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const total = clashConnections?.at(-1)?.uploadTotal

  return (
    <SparklineCard
      data={padData(
        clashTraffic?.map((item) => item.up),
        MAX_TRAFFIC_HISTORY,
      )}
    >
      <SparklineCardTitle icon={ArrowUpwardRounded}>
        {m.dashboard_widget_traffic_upload()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashTraffic?.at(-1)?.up ?? 0)}/s
      </SparklineCardContent>

      <SparklineCardBottom>
        {total !== undefined &&
          m.dashboard_widget_traffic_total({
            value: filesize(total),
          })}
      </SparklineCardBottom>
    </SparklineCard>
  )
}

export function ConnectionsWidget() {
  const {
    query: { data: clashConnections },
  } = useClashConnections()

  return (
    <SparklineCard
      data={padData(
        clashConnections?.map((item) => item.connections?.length ?? 0),
        MAX_CONNECTIONS_HISTORY,
      )}
    >
      <SparklineCardTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_connections()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {clashConnections?.at(-1)?.connections?.length ?? 0}
      </SparklineCardContent>

      <SparklineCardBottom />
    </SparklineCard>
  )
}

export function MemoryWidget() {
  const { data: clashMemory } = useClashMemory()

  return (
    <SparklineCard
      data={padData(
        clashMemory?.map((item) => item.inuse),
        MAX_MEMORY_HISTORY,
      )}
    >
      <SparklineCardTitle icon={MemoryOutlineRounded}>
        {m.dashboard_widget_memory()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashMemory?.at(-1)?.inuse ?? 0)}
      </SparklineCardContent>

      <SparklineCardBottom />
    </SparklineCard>
  )
}
