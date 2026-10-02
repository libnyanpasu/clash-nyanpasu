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

function StatCard({
  icon: Icon,
  label,
  value,
  detail,
  hint,
}: {
  icon: ComponentType<SVGProps<SVGSVGElement>>
  label: string
  value: string
  detail?: ReactNode
  /** What the figure means; the card is then a focusable tooltip trigger. */
  hint?: string
}) {
  const card = (
    <Card
      className={cn(
        'flex gap-3 p-3 sm:gap-4 sm:p-4',
        hint && 'focus-visible:ring-primary outline-none focus-visible:ring-2',
      )}
      data-slot="traffic-stat"
      tabIndex={hint ? 0 : undefined}
    >
      <div className="bg-secondary-container text-on-secondary-container grid size-10 shrink-0 place-content-center rounded-2xl sm:size-12">
        <Icon className="size-5 sm:size-6" />
      </div>

      <div className="min-w-0 [overflow-wrap:anywhere]">
        <p className="text-on-surface-variant text-xs">{label}</p>

        <p className="text-xl font-medium tabular-nums sm:text-2xl">{value}</p>

        {detail}
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

export default function StatCards({ report }: { report?: TrafficReport }) {
  const traffic = (bytes?: number) =>
    bytes === undefined ? '—' : parseTraffic(bytes).join(' ')

  const bytes = report?.total.bytes

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 sm:gap-4 xl:grid-cols-6">
      <StatCard
        icon={SwapVertRounded}
        label={m.traffic_stat_total()}
        value={traffic(bytes && bytes.upload + bytes.download)}
        detail={
          bytes && (
            <p className="text-on-surface-variant flex flex-wrap gap-x-2 text-xs tabular-nums">
              <span className="whitespace-nowrap">
                <span className="text-outline mr-1">↑</span>
                {traffic(bytes.upload)}
              </span>

              <span className="whitespace-nowrap">
                <span className="text-outline mr-1">↓</span>
                {traffic(bytes.download)}
              </span>
            </p>
          )
        }
      />

      <StatCard
        icon={CableRounded}
        label={m.traffic_stat_connections()}
        value={report?.total.connections.toLocaleString() ?? '—'}
        hint={m.traffic_stat_connections_hint()}
      />

      {(
        [
          ['source', DevicesRounded],
          ['inbound', LoginRounded],
          ['target', DnsRounded],
          ['exit', CallSplitRounded],
        ] as const
      ).map(([dimension, icon]) => (
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
