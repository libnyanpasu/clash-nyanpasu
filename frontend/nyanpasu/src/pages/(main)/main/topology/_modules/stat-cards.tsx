import CableRounded from '~icons/material-symbols/cable-rounded'
import SpeedRounded from '~icons/material-symbols/speed-rounded'
import SwapVertRounded from '~icons/material-symbols/swap-vert-rounded'
import type { ComponentType, ReactNode, SVGProps } from 'react'
import { Card, type CardVariantsProps } from '@nyanpasu/ui/card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import { dimensionName } from '@/utils/traffic-usage'
import type { TrafficReport } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import { RANKINGS } from './traffic-context'

/**
 * A label over its figure, one line each, so that every card has the same
 * height and no card stretches the others in its row.
 */
function StatCard({
  icon: Icon,
  label,
  value,
  detail,
  hint,
  variant,
  className,
}: {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  label: string
  value: string
  /** Beside the figure, no taller than its line. */
  detail?: ReactNode
  /** What the figure means; the card is then a focusable tooltip trigger. */
  hint?: string
  variant?: CardVariantsProps['variant']
  className?: string
}) {
  const card = (
    <Card
      variant={variant}
      className={cn(
        'flex items-center gap-2 p-2',
        hint && 'focus-visible:ring-primary outline-none focus-visible:ring-2',
        className,
      )}
      data-slot="traffic-stat"
      tabIndex={hint ? 0 : undefined}
    >
      <div className="bg-secondary-container text-on-secondary-container grid size-8 shrink-0 place-content-center rounded-xl">
        <Icon className="size-4" />
      </div>

      <div className="min-w-0">
        <p className="text-on-surface-variant truncate text-xs" title={label}>
          {label}
        </p>

        <div className="flex min-w-0 items-end gap-3">
          <p className="truncate text-lg font-medium tabular-nums">{value}</p>

          {detail}
        </div>
      </div>
    </Card>
  )

  return hint ? (
    <Tooltip>
      <TooltipTrigger asChild>{card}</TooltipTrigger>

      <TooltipContent className="max-w-80 rounded-2xl">{hint}</TooltipContent>
    </Tooltip>
  ) : (
    card
  )
}

/** Upload over download, beside a figure. */
function UpDown({
  upload,
  download,
  className,
}: {
  upload: string
  download: string
  className?: string
}) {
  return (
    <p
      className={cn(
        'text-on-surface-variant shrink-0 text-xs leading-3.5 whitespace-nowrap tabular-nums',
        className,
      )}
    >
      <span className="block">
        <span className="text-outline mr-1">↑</span>
        {upload}
      </span>

      <span className="block">
        <span className="text-outline mr-1">↓</span>
        {download}
      </span>
    </p>
  )
}

export default function StatCards({ report }: { report?: TrafficReport }) {
  const traffic = (bytes?: number) =>
    bytes === undefined ? '—' : parseTraffic(bytes).join(' ')
  const speed = (bytes?: number) =>
    bytes === undefined ? '—' : `${traffic(bytes)}/s`

  const bytes = report?.total.bytes
  // Closed connections have no rate.
  const rate = report?.current_rate ?? undefined

  return (
    <div className="@container flex flex-col gap-2">
      <div className="grid grid-cols-2 gap-2 @md:grid-cols-4">
        <StatCard
          icon={SwapVertRounded}
          label={m.traffic_stat_total()}
          value={traffic(bytes && bytes.upload + bytes.download)}
          variant="raised"
          detail={
            bytes && (
              <UpDown
                upload={traffic(bytes.upload)}
                download={traffic(bytes.download)}
              />
            )
          }
          className="col-span-2"
        />

        <StatCard
          icon={CableRounded}
          label={m.traffic_stat_connections()}
          value={report?.total.connections.toLocaleString() ?? '—'}
          hint={m.traffic_stat_connections_hint()}
        />

        <StatCard
          icon={SpeedRounded}
          label={m.traffic_stat_rate()}
          value={speed(rate && rate.upload + rate.download)}
          detail={
            rate && (
              // Only a wide card has room for both beside the figure.
              <UpDown
                upload={speed(rate.upload)}
                download={speed(rate.download)}
                className="hidden @5xl:block"
              />
            )
          }
        />
      </div>

      {/* One count per ranking, in the order of the ranking cards. */}
      <div className="grid grid-cols-[repeat(auto-fit,minmax(7.5rem,1fr))] gap-2">
        {RANKINGS.map(({ dimension, icon }) => (
          <StatCard
            key={dimension}
            icon={icon}
            label={dimensionName(dimension)}
            value={
              report?.rankings
                .find((ranking) => ranking.dimension === dimension)
                ?.distinct.toLocaleString() ?? '—'
            }
          />
        ))}
      </div>
    </div>
  )
}
