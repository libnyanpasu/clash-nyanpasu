import { AnimatePresence, motion } from 'motion/react'
import { useMemo } from 'react'
import { useMedia } from 'react-use'
import worldMap from '@/assets/maps/world-map.json'
import { m } from '@/paraglide/messages'
import {
  largest,
  regionName,
  usageAmount,
  usageValue,
} from '@/utils/traffic-usage'
import type { Metric, Topology, Usage } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'

const centers: Readonly<Record<string, number[]>> = worldMap.centers
const geographicRegions: ReadonlySet<string> = new Set(Object.keys(centers))

export default function GeographyView({
  topology,
  metric,
  selection,
  onSelect,
}: {
  /** The report's source and destination regions, nothing merged. */
  topology: Topology | null | undefined
  metric: Metric
  selection?: string
  onSelect: (code: string) => void
}) {
  const reducedMotion = useMedia('(prefers-reduced-motion: reduce)', false)
  const transition = {
    duration: reducedMotion ? 0 : 0.32,
    ease: [0.2, 0, 0, 1] as const,
  }
  const { regions, routes, total, unknown } = useMemo(() => {
    const nodes = topology?.nodes ?? []
    const keyOf = new Map(nodes.map((node) => [node.id, node.key]))
    // The destination totals are the second layer's nodes.
    const regions = nodes
      .filter((node) => node.layer === 1 && node.key !== null)
      .map((node) => ({ code: node.key!, usage: node.usage }))
      .sort(
        (a, b) =>
          usageValue(b.usage, metric) - usageValue(a.usage, metric) ||
          a.code.localeCompare(b.code),
      )
    const routes = (topology?.edges ?? [])
      .map((edge) => ({
        source: keyOf.get(edge.source),
        destination: keyOf.get(edge.target),
        usage: edge.usage,
      }))
      .filter(
        (
          route,
        ): route is typeof route & { source: string; destination: string } =>
          !!route.source &&
          !!route.destination &&
          route.source !== route.destination &&
          geographicRegions.has(route.source) &&
          geographicRegions.has(route.destination),
      )
    const total = regions.reduce((sum, { usage }) => sum + usage.connections, 0)
    const unknown = regions
      .filter(({ code }) => !geographicRegions.has(code))
      .reduce((sum, { usage }) => sum + usage.connections, 0)

    return { regions, routes, total, unknown }
  }, [topology, metric])
  // No topology yet is a request in flight; a topology without regions is empty.
  const hint =
    topology && total === 0
      ? m.traffic_empty()
      : total > 0 && unknown === total
        ? m.topology_geo_empty()
        : undefined
  const value = (usage: Usage) => usageValue(usage, metric)
  const maximum = largest(
    regions.map(({ usage }) => value(usage)),
    1,
  )
  const select = (code: string) => onSelect(code)
  const amount = (usage: Usage) => usageAmount(usage, metric)
  // Bound the number of animated arcs; every region remains in the list below.
  const arcs = routes
    .filter((route) => !selection || route.destination === selection)
    .sort(
      (a, b) =>
        value(b.usage) - value(a.usage) ||
        `${a.source}:${a.destination}`.localeCompare(
          `${b.source}:${b.destination}`,
        ),
    )
    .slice(0, 32)

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
        <h2 className="font-medium">{m.topology_geo_title()}</h2>
        <span className="text-on-surface-variant text-xs">
          {m.topology_geo_coverage({
            known: total - unknown,
            total,
          })}
        </span>
      </div>
      <div className="bg-secondary-container/15 relative overflow-hidden rounded-2xl">
        <svg
          viewBox="0 0 960 460"
          className="w-full"
          role="group"
          aria-label={m.topology_geo_title()}
        >
          <path
            d={worldMap.graticule}
            fill="none"
            className="stroke-outline-variant"
            strokeWidth="0.6"
            opacity="0.45"
            aria-hidden="true"
          />
          <path
            d={worldMap.outline}
            className="fill-surface-variant stroke-outline-variant"
            strokeWidth="0.5"
            aria-hidden="true"
          />
          <AnimatePresence initial={false}>
            {arcs.map((route) => {
              const [x1, y1] = centers[route.source]
              const [x2, y2] = centers[route.destination]
              // A regional arc is schematic; it is not a measured network route.
              const d = `M ${x1} ${y1} Q ${(x1 + x2) / 2} ${Math.max(8, Math.min(y1, y2) - Math.min(100, Math.abs(x2 - x1) * 0.2))} ${x2} ${y2}`
              return (
                <motion.path
                  key={`${route.source}:${route.destination}`}
                  d={d}
                  fill="none"
                  className="stroke-tertiary"
                  aria-hidden="true"
                  initial={{ opacity: 0 }}
                  animate={{
                    opacity: 0.55,
                    strokeWidth: 1 + (value(route.usage) / maximum) * 3,
                  }}
                  exit={{ opacity: 0 }}
                  transition={transition}
                />
              )
            })}
          </AnimatePresence>
          <AnimatePresence initial={false}>
            {regions
              .filter((region) => geographicRegions.has(region.code))
              .map((region) => {
                const [x, y] = centers[region.code]
                const active = selection === region.code
                return (
                  <motion.g
                    key={region.code}
                    role="button"
                    tabIndex={0}
                    aria-label={`${regionName(region.code)}: ${amount(region.usage)}`}
                    aria-pressed={active}
                    onClick={() => select(region.code)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault()
                        select(region.code)
                      }
                    }}
                    className="group cursor-pointer outline-none"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: selection && !active ? 0.35 : 1 }}
                    exit={{ opacity: 0 }}
                    transition={transition}
                  >
                    <title>
                      {regionName(region.code)} · {amount(region.usage)}
                    </title>
                    <circle
                      cx={x}
                      cy={y}
                      r="13"
                      fill="transparent"
                      className="group-focus-visible:stroke-primary"
                      strokeWidth="2"
                    />
                    <motion.circle
                      cx={x}
                      cy={y}
                      initial={false}
                      animate={{
                        r: 4 + Math.sqrt(value(region.usage) / maximum) * 7,
                      }}
                      transition={transition}
                      className={cn(
                        'stroke-surface',
                        active ? 'fill-tertiary' : 'fill-primary',
                      )}
                      strokeWidth="2"
                    />
                    {active && (
                      <text
                        x={x > 800 ? x - 17 : x + 17}
                        y={y - 12}
                        textAnchor={x > 800 ? 'end' : 'start'}
                        className="fill-on-surface stroke-surface text-xs font-medium"
                        strokeWidth="3"
                        paintOrder="stroke"
                      >
                        {regionName(region.code)}
                      </text>
                    )}
                  </motion.g>
                )
              })}
          </AnimatePresence>
        </svg>
        {hint && (
          <p className="bg-surface/90 text-on-surface-variant absolute inset-x-4 bottom-4 rounded-2xl p-3 text-center text-sm">
            {hint}
          </p>
        )}
      </div>
      <div
        className="flex max-h-40 flex-wrap gap-2 overflow-y-auto"
        aria-label={m.topology_geo_regions()}
      >
        {regions.map((region) => (
          <button
            key={region.code}
            type="button"
            aria-pressed={selection === region.code}
            onClick={() => select(region.code)}
            className={cn(
              'focus-visible:ring-primary rounded-full px-3 py-2 text-xs outline-none focus-visible:ring-2',
              selection === region.code
                ? 'bg-tertiary-container text-on-tertiary-container'
                : 'bg-secondary-container/50 text-on-secondary-container',
            )}
          >
            {regionName(region.code)}{' '}
            <span className="ml-2 tabular-nums opacity-75">
              {amount(region.usage)}
            </span>
          </button>
        ))}
      </div>
      <p className="text-on-surface-variant text-xs leading-relaxed">
        {m.topology_geo_caption()}
      </p>
      <p className="text-on-surface-variant text-[10px]">
        {m.topology_geo_credit()}
      </p>
    </div>
  )
}
