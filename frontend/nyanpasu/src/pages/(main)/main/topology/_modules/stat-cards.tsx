import AppsRounded from '~icons/material-symbols/apps-rounded'
import CableRounded from '~icons/material-symbols/cable-rounded'
import CallSplitRounded from '~icons/material-symbols/call-split-rounded'
import DevicesRounded from '~icons/material-symbols/devices-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import LoginRounded from '~icons/material-symbols/login-rounded'
import SwapVertRounded from '~icons/material-symbols/swap-vert-rounded'
import type { ComponentType, ReactNode, SVGProps } from 'react'
import { Card } from '@nyanpasu/ui/card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@nyanpasu/ui/tooltip'
import { m } from '@/paraglide/messages'
import parseTraffic from '@/utils/parse-traffic'
import { dimensionName } from '@/utils/traffic-usage'
import type { Dimension, TrafficReport } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'

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
  className,
}: {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  label: string
  value: string
  /** Beside the figure, no taller than its line. */
  detail?: ReactNode
  /** What the figure means; the card is then a focusable tooltip trigger. */
  hint?: string
  className?: string
}) {
  const card = (
    <Card
      className={cn(
        'flex items-center gap-3 p-3',
        hint && 'focus-visible:ring-primary outline-none focus-visible:ring-2',
        className,
      )}
      data-slot="traffic-stat"
      tabIndex={hint ? 0 : undefined}
    >
      <div className="bg-secondary-container text-on-secondary-container grid size-10 shrink-0 place-content-center rounded-2xl">
        <Icon className="size-5" />
      </div>

      <div className="min-w-0">
        {/* At eight columns, a long label is cut short. */}
        <p className="text-on-surface-variant truncate text-xs" title={label}>
          {label}
        </p>

        <div className="flex min-w-0 items-end gap-3">
          <p className="truncate text-xl font-medium tabular-nums">{value}</p>

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

const distinctOf = (report: TrafficReport, dimension: Dimension) =>
  report.rankings
    .find((ranking) => ranking.dimension === dimension)
    ?.distinct.toLocaleString() ?? '—'

// One count per ranking card below. With the traffic card two columns wide,
// the rows are full at two, four and eight columns.
const DISTINCT = [
  ['source', DevicesRounded],
  ['inbound', LoginRounded],
  ['target', DnsRounded],
  ['exit', CallSplitRounded],
  ['process', AppsRounded],
] as const

export default function StatCards({ report }: { report?: TrafficReport }) {
  const traffic = (bytes?: number) =>
    bytes === undefined ? '—' : parseTraffic(bytes).join(' ')

  const bytes = report?.total.bytes

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 sm:gap-4 xl:grid-cols-8">
      <StatCard
        icon={SwapVertRounded}
        label={m.traffic_stat_total()}
        value={traffic(bytes && bytes.upload + bytes.download)}
        detail={
          bytes && (
            <p className="text-on-surface-variant shrink-0 text-xs leading-3.5 whitespace-nowrap tabular-nums">
              <span className="block">
                <span className="text-outline mr-1">↑</span>
                {traffic(bytes.upload)}
              </span>

              <span className="block">
                <span className="text-outline mr-1">↓</span>
                {traffic(bytes.download)}
              </span>
            </p>
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

      {DISTINCT.map(([dimension, icon]) => (
        <StatCard
          key={dimension}
          icon={icon}
          label={dimensionName(dimension)}
          value={report ? distinctOf(report, dimension) : '—'}
        />
      ))}
    </div>
  )
}
